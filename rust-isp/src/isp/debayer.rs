use tracing::instrument;
use std::path::PathBuf;

use crate::image::Convolvable;
use crate::image::{Image, RGBImage};

const KERNEL: [[f32; 3]; 3] = [
    [0.0, 0.25, 0.0],
    [0.25, 0.0, 0.25],
    [0.0, 0.25, 0.0],
];

#[instrument(skip(image))]
pub fn debayer(image: &Image, dir: PathBuf) -> RGBImage {
    let red = extract_channel(&image, 0, 0);
    //debug!(?red.width, ?red.height, "extracted red channel");
    //save_single_channel(&red, dir.join("debayer_red.tga"), RGBChannel::Red);
    let red = bilinear_upsample(&red, 2);
    //save_single_channel(&red, dir.join("debayer_3x3_bilinear_red.tga"), RGBChannel::Red);

    let blue = extract_channel(&image, 1, 1);
    //save_single_channel(&blue, dir.join("debayer_blue.tga"), RGBChannel::Blue);
    let blue = bilinear_upsample(&blue, 2);
    //save_single_channel(&blue, dir.join("debayer_3x3_bilinear_blue.tga"), RGBChannel::Blue);

    let mut green = image.convolve::<3,3>(&KERNEL);
    for y in 0..image.height {
        for x in 0..image.width {
            if (x % 2 == 0 && y % 2 == 1) || (x % 2 == 1 && y % 2 == 0) {
                let idx = y * image.width + x;
                green.data[idx] = image.data[idx];
            }
        }
    }

    //save_single_channel(&green, dir.join("debayer_3x3_bilinear_green.tga"), RGBChannel::Green);

    let mut rgb_data = Vec::with_capacity(image.width * image.height);
    for i in 0..(image.width * image.height) {
        let pixel = [
            red.data[i],
            green.data[i],
            blue.data[i],
        ];
        rgb_data.push(pixel);
    }

    RGBImage {
        width: image.width,
        height: image.height,
        data: rgb_data,
    }

}

#[instrument(skip(image))]
fn extract_channel(image: &Image, x_offset: usize, y_offset: usize) -> Image {
    let c_width = (image.width - x_offset + 1) / 2;
    let c_height = (image.height - y_offset + 1) / 2;
    let mut data = Vec::with_capacity(c_width * c_height);

    for y in (y_offset..image.height).step_by(2) {
        for x in (x_offset..image.width).step_by(2) {
            let idx = y * image.width + x;
            data.push(image.data[idx]);
        }
    }

    Image::new(c_width, c_height, data)
}

// - https://en.wikipedia.org/wiki/Bilinear_interpolation
// - https://bartwronski.com/2021/02/15/bilinear-down-upsampling-pixel-grids-and-that-half-pixel-offset/
#[instrument(skip(image))]
fn bilinear_upsample(image: &Image, factor: usize) -> Image {
    let u_width = image.width * factor;
    let u_height = image.height * factor;
    let mut upsampled = Image::new(u_width, u_height, vec![0f32; u_width * u_height]);

    // should always be about ~0.5
    let x_ratio = (image.width - 1) as f32 / (u_width - 1) as f32;
    let y_ratio = (image.height - 1) as f32 / (u_height - 1) as f32;

    for y in 0..upsampled.height {
        let y0 = (y as f32 * y_ratio).floor() as usize;
        let y1 = (y0 + 1).min(image.height - 1);
        let dy = (y as f32 * y_ratio) - y0 as f32;

        for x in 0..upsampled.width {
            let x0 = (x as f32 * x_ratio).floor() as usize;
            let x1 = (x0 + 1).min(image.width - 1);
            let dx = (x as f32 * x_ratio) - x0 as f32;

            let idx_00 = y0 * image.width + x0;
            let idx_01 = y0 * image.width + x1;
            let idx_10 = y1 * image.width + x0;
            let idx_11 = y1 * image.width + x1;

            let value = 
                image.data[idx_00] * (1.0 - dx) * (1.0 - dy) +
                image.data[idx_01] * dx * (1.0 - dy) +
                image.data[idx_10] * (1.0 - dx) * dy +
                image.data[idx_11] * dx * dy;

            upsampled.data[y * upsampled.width + x] = value;
        }
    }
    upsampled
}
