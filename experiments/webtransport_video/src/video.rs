use std::{collections::VecDeque, num::Wrapping, sync::Arc};

use color_eyre::{eyre::WrapErr as _, Result};
use derive_more::{AsRef, Deref, Into};
use tokio_util::sync::CancellationToken;
use tracing::trace;

const WIDTH: usize = 640;
const HEIGHT: usize = 480;
const COLOR_TYPE: png::ColorType = png::ColorType::Rgb;

static EMPTY_BUF: &[u8] = &[0; WIDTH * HEIGHT * 3]; // Couldn't use a const :(

pub struct VideoTaskHandle {
    pub frame_rx: tokio::sync::watch::Receiver<EncodedPng>,
    pub task_handle: tokio::task::JoinHandle<()>,
}

impl VideoTaskHandle {
    pub fn spawn(cancel: CancellationToken) -> Self {
        let video = Video::new();
        let initial_empty_frame = {
            let mut frame = Vec::with_capacity(1024);
            Video::empty_frame(&mut frame);
            EncodedPng(Arc::new(frame))
        };

        let (tx, rx) = tokio::sync::watch::channel(initial_empty_frame);
        let task_handle =
            tokio::task::spawn_blocking(move || video_task(tx, cancel, video));

        Self {
            frame_rx: rx,
            task_handle,
        }
    }
}

fn video_task(
    tx: tokio::sync::watch::Sender<EncodedPng>,
    cancel: CancellationToken,
    mut video: Video,
) {
    let _cancel_guard = cancel.clone().drop_guard();

    // Holds arcs swapped out of the tokio channel. Popped only when strong count
    // indicates no further references by network tasks.
    let mut arc_pool: VecDeque<EncodedPng> = VecDeque::with_capacity(4);
    // Holds arcs that were promoted to vecs after checking their count.
    let mut vec_pool: Vec<Vec<u8>> = vec![Vec::with_capacity(1024)];

    while !cancel.is_cancelled() {
        // We bound the maximum number of arcs to scan, to avoid this task slowing
        // down too much due to too many outstanding connections holding on to arcs.
        const N_ARC_TO_SCAN: usize = 256;
        let n_arcs_before_pop = arc_pool.len();
        pop_arcs(N_ARC_TO_SCAN, &mut arc_pool, &mut vec_pool);
        trace!("reaped {} arcs", n_arcs_before_pop - arc_pool.len());
        let mut png_buf: Vec<u8> =
            vec_pool.pop().unwrap_or_else(|| Vec::with_capacity(1024));

        png_buf.clear();
        video
            .next_png(png_buf.as_mut())
            .expect("error while producing video");
        let encoded = EncodedPng(Arc::new(png_buf));

        // This could block if anyone is borrowing the channel, so be sure we dont for
        // long.
        let swapped = tx.send_replace(encoded);
        arc_pool.push_back(swapped);
    }
}

/// Pops at most `max_num_to_pop` arcs, if their strong counts are low enough.
/// Places the popped inner values in `out`, calling `popped.into()` in the process.
fn pop_arcs<T, U>(max_num_to_pop: usize, arcs: &mut VecDeque<T>, out: &mut Vec<U>)
where
    T: Into<Arc<U>> + AsRef<Arc<U>>,
{
    for _ in 0..max_num_to_pop {
        let Some(candidate) = arcs.pop_front() else {
            return;
        };
        if Arc::strong_count(candidate.as_ref()) != 1 {
            // there are other references, return to queue.
            arcs.push_back(candidate);
            continue;
        }
        // No other references, move to `out`.
        let inner = Arc::into_inner(candidate.into())
            .expect("we just checked the reference count, should be infallible");
        out.push(inner);
    }
}

/// Newtype on a vec, to indicate that this contains a png-encoded image.
#[derive(Debug, Into, AsRef, Clone, Deref)]
pub struct EncodedPng(pub Arc<Vec<u8>>);

impl EncodedPng {
    /// Equivalent to [`Self::clone`] but is more explicit that this operation is cheap.
    pub fn clone_cheap(&self) -> Self {
        EncodedPng(Arc::clone(&self.0))
    }
}

impl AsRef<[u8]> for EncodedPng {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

/// Generates a video feed.
pub struct Video {
    frame_buffer: Vec<u8>,
    i: Wrapping<u8>,
}

impl Video {
    pub fn new() -> Self {
        let frame_buffer = vec![0u8; WIDTH * HEIGHT * COLOR_TYPE.samples()];
        Self {
            frame_buffer,
            i: Wrapping(0),
        }
    }

    /// Renders an emtpy frame and encodes it as a png, placing it in `png_out`.
    /// Using an out-param allows for 1 fewer copy.
    pub fn empty_frame(png: &mut Vec<u8>) {
        png.clear();
        encode_frame(png, EMPTY_BUF).expect("empty frame should always encode");
    }

    /// Renders the next frame and encodes it as a png, placing it in `png_out`.
    /// Using an out-param allows for 1 fewer copy.
    pub fn next_png(&mut self, png_out: &mut Vec<u8>) -> Result<()> {
        self.i += Wrapping(1);
        png_out.clear();
        draw_image(&mut self.frame_buffer, self.i.0);
        encode_frame(png_out, &self.frame_buffer).wrap_err("failed to encode png")
    }
}

fn encode_frame(png_buffer: &mut Vec<u8>, raw_frame: &[u8]) -> Result<()> {
    let mut encoder = png::Encoder::new(
        png_buffer,
        WIDTH.try_into().unwrap(),
        HEIGHT.try_into().unwrap(),
    );
    assert_eq!(raw_frame.len(), WIDTH * HEIGHT * COLOR_TYPE.samples());
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_compression(png::Compression::Fast);
    let mut encoder = encoder
        .write_header()
        .wrap_err("failed to write png header")?;
    encoder
        .write_image_data(raw_frame)
        .wrap_err("failed to write png data")?;
    Ok(())
}

/// Fills the buffer with a simple pattern
fn draw_image(raw_frame: &mut [u8], i: u8) {
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let idx = (y * WIDTH + x) * COLOR_TYPE.samples();

            // Create a gradient pattern
            let r = (x as f32 / WIDTH as f32 * 255.0) as u8;
            let g = (y as f32 / HEIGHT as f32 * 255.0) as u8;
            let b = i;

            raw_frame[idx] = r;
            raw_frame[idx + 1] = g;
            raw_frame[idx + 2] = b;
        }
    }
}
