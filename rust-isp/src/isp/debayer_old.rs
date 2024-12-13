use crate::image::Image;

use crate::image::{Convolvable, RGBImage};

const KERNELS: [[[f32; 3]; 3]; 4] = [
    [
        [0.0, 0.25, 0.0],
        [0.25, 0.0, 0.25],
        [0.0, 0.25, 0.0],
    ],
    [
        [0.25, 0.0, 0.25],
        [0.0, 0.0, 0.0],
        [0.25, 0.0, 0.25],
    ],
    [
        [0.0, 0.0, 0.0],
        [0.5, 0.0, 0.5],
        [0.0, 0.0, 0.0],
    ],
    [
        [0.0, 0.5, 0.0],
        [0.0, 0.0, 0.0],
        [0.0, 0.5, 0.0],
    ],
];

const INDEX: [[[usize; 2]; 2]; 3] = [
    // R channel
    [
        [4, 2],
        [3, 1],
    ],
    // G channel
    [
        [0, 4],
        [4, 0],
    ],
    // B channel
    [
        [1, 3],
        [2, 4],
    ],
];

pub fn debayer(image: &Image) -> RGBImage {
    let mut channels = Vec::with_capacity(KERNELS.len());
    for kernel in KERNELS.iter() {
        let convolved = image.convolve::<3, 3>(kernel);
        channels.push(convolved);
    }
    channels.push(image.clone());

    RGBImage::from_channels::<2, 2>(&channels, &INDEX)
}
