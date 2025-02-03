use std::marker::PhantomData;

use crate::frame_format::Pixel;

use crate::{
    error::ErrorCode,
    sys::{self, frame_t, seekframe_t},
};

type Result<T> = std::result::Result<T, ErrorCode>;

/// Contains one or more `Frame`s, each of which may be in a different format.
#[derive(Debug)]
pub struct FrameContainer<'a> {
    ptr: *mut frame_t,
    _phantom: PhantomData<&'a mut frame_t>,
}
impl FrameContainer<'_> {
    /// Creates a frame container with lifetime `'a`.
    pub(crate) unsafe fn new(ptr: *mut frame_t) -> Self {
        Self {
            ptr,
            _phantom: PhantomData,
        }
    }
}

impl FrameContainer<'_> {
    pub fn get_frame<P: Pixel>(&self) -> Result<Frame<'_, P>> {
        let mut frame_ptr = std::ptr::null_mut();
        let err = unsafe {
            sys::frame_get_frame_by_format(
                self.ptr,
                sys::frame_format_t(P::FRAME_FORMAT as u32),
                &mut frame_ptr,
            )
        };
        ErrorCode::result_from_sys(err)?;
        Ok(Frame::new(frame_ptr))
    }
}

unsafe impl Send for FrameContainer<'_> {}
unsafe impl Sync for FrameContainer<'_> {}

pub struct Frame<'a, P> {
    ptr: *const seekframe_t,
    pixels: &'a [P],
}
impl<P: Pixel> Frame<'_, P> {
    fn new(ptr: *const seekframe_t) -> Self {
        let nbytes = unsafe { sys::seekframe_get_data_size(ptr) };
        let data_ptr = unsafe { sys::seekframe_get_data(ptr) }.cast::<u8>();

        #[cfg(debug_assertions)]
        {
            let depth = unsafe { sys::seekframe_get_pixel_depth(ptr) };
            let channels = unsafe { sys::seekframe_get_channels(ptr) };
            assert_eq!(P::PIXEL_DEPTH as usize, depth);
            assert_eq!(P::CHANNELS as usize, channels);

            assert_ne!(nbytes, 0);
            assert_ne!(data_ptr, core::ptr::null_mut());
        }

        let bytes = unsafe { std::slice::from_raw_parts(data_ptr, nbytes) };

        let pixels = bytemuck::cast_slice(bytes);
        Self { ptr, pixels }
    }

    pub fn width(&self) -> usize {
        unsafe { sys::seekframe_get_width(self.ptr) }
    }

    pub fn height(&self) -> usize {
        unsafe { sys::seekframe_get_height(self.ptr) }
    }

    pub fn channels(&self) -> usize {
        unsafe { sys::seekframe_get_channels(self.ptr) }
    }

    pub fn pixel_depth(&self) -> usize {
        unsafe { sys::seekframe_get_pixel_depth(self.ptr) }
    }

    pub fn pixels(&self) -> &[P] {
        self.pixels
    }
}

unsafe impl<P> Send for Frame<'_, P> {}
unsafe impl<P> Sync for Frame<'_, P> {}
