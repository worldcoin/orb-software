use crate::image::Image;
use tracing::instrument;

pub mod canny;
pub mod sobel;
pub mod canny2;

#[instrument(skip(image), level = "debug")]
pub fn threshold(image: &mut Image, threshold: f32) {
    image.data.iter_mut().for_each(|pixel| *pixel = if *pixel > threshold { 1023.0 } else { 0.0 });
}

#[instrument(skip(image), level = "debug")]
pub fn double_threshold(image: &mut Image, high_threshold: f32, low_threshold: f32) {
    image.data.iter_mut().for_each(|pixel| {
        *pixel = if *pixel > high_threshold {
            1023.0
        } else if *pixel > low_threshold {
            300.0
        } else {
            0.0
        }
    });
}
