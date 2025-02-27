use eyre::{bail, Result};
use gstreamer::{
    prelude::{Cast as _, ElementExt as _, GstBinExtManual as _},
    ElementFactory, Pipeline,
};
use gstreamer_app::{AppSink, AppSrc};
use gstreamer_video::{VideoFormat, VideoFrameRef, VideoInfo};
use tracing::trace;

pub struct GstreamerJpegEncoder {
    pipeline: Pipeline,
    appsrc: AppSrc,
    appsink: AppSink,
    video_info: VideoInfo,
}

impl GstreamerJpegEncoder {
    /// NOTE: will error if it is not a supported frame format. See
    /// <https://docs.nvidia.com/metropolis/deepstream/dev-guide/text/DS_plugin_gst-nvvideoconvert.html>
    pub fn new(video_info: VideoInfo) -> Result<Self> {
        {
            use VideoFormat::*;
            match video_info.format() {
                Nv12 | Rgb | Bgr | Gray8 | Rgba | Bgrx => (),
                _ => bail!("unsupported frame format"),
            }
        }
        let pipeline = Pipeline::with_name("livestream");
        let appsrc = AppSrc::builder().caps(&video_info.to_caps()?).build();
        appsrc.set_block(false);
        appsrc.set_stream_type(gstreamer_app::AppStreamType::Stream);
        appsrc.set_is_live(true);
        let nvvidconv = ElementFactory::make("nvvidconv").build()?;
        let nvvjpegenc = ElementFactory::make("nvjpegenc").build()?;
        let jpegparse = ElementFactory::make("jpegparse").build()?;

        let appsink = AppSink::builder().max_buffers(1).drop(true).build();

        pipeline.add_many([
            appsrc.upcast_ref(),
            &nvvidconv,
            &nvvjpegenc,
            &jpegparse,
            appsink.upcast_ref(),
        ])?;
        gstreamer::Element::link_many([
            appsrc.upcast_ref(),
            &nvvidconv,
            &nvvjpegenc,
            &jpegparse,
            appsink.upcast_ref(),
        ])?;
        pipeline.set_state(gstreamer::State::Playing)?;

        Ok(Self {
            pipeline,
            appsrc,
            appsink,
            video_info,
        })
    }

    pub fn encode(&self, frame: &[u8], jpg_out: &mut Vec<u8>) -> Result<()> {
        jpg_out.clear();
        let mut buffer = gstreamer::Buffer::with_size(self.video_info.size())
            .expect("failed to create a new gstreamer buffer");
        {
            let buffer = buffer.get_mut().unwrap();
            let mut video_frame =
                VideoFrameRef::from_buffer_ref_writable(buffer, &self.video_info)
                    .unwrap();
            let plane_data = video_frame.plane_data_mut(0).unwrap();
            unsafe {
                std::ptr::copy_nonoverlapping(
                    frame.as_ptr(),
                    plane_data.as_mut_ptr(),
                    frame.len(),
                );
            }
        }
        self.appsrc.push_buffer(buffer)?;
        // NOTE(@thebutlah): we could have used the callback API but I think this way
        // is simpler, since it keeps everything synchronous and the api for the caller
        // is simple.
        let sample = self.appsink.pull_sample()?;
        trace!(?sample, "got sample");
        let buffer = sample.buffer().expect("sample should never be empty");
        let mapped = buffer.map_readable()?;
        jpg_out.extend_from_slice(&mapped);

        Ok(())
    }
}

impl Drop for GstreamerJpegEncoder {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gstreamer::State::Null);
    }
}
