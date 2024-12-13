use std::{f32::consts::PI, path::PathBuf};

use tracing::instrument;

use crate::gaussian;
use crate::image::{Convolvable, Image};

pub const SOBEL_KERNELS: [[[f32; 3]; 3]; 2] = [
    [
        [-1.0, 0.0, 1.0],
        [-2.0, 0.0, 2.0],
        [-1.0, 0.0, 1.0],
    ],
    [
        [-1.0, -2.0, -1.0],
        [0.0, 0.0, 0.0],
        [1.0, 2.0, 1.0]
    ]
];

pub fn canny(
    image: &Image,
    low_threshold: f32,
    high_threshold: f32,
) -> Image {
    let blurred_image = gaussian::blur(image);

    let (gradient_magnitude, gradient_direction) = sobel(&blurred_image);
    gradient_magnitude.save_gray_tga(PathBuf::from("test11.d/edge_sobel_mag.tga"));
    gradient_direction.save_gray_tga(PathBuf::from("test11.d/edge_sobel_dir.tga"));

    let mut non_max_suppressed = non_maximum_suppression(&gradient_magnitude, &gradient_direction);

    crate::edge::double_threshold(&mut non_max_suppressed, low_threshold, high_threshold);

    let edges = edge_tracking_by_hysteresis(&non_max_suppressed);

    edges
}

#[instrument(skip(image))]
pub fn sobel(image: &Image) -> (Image, Image) {
    let grad_x = image.convolve::<3, 3>(&SOBEL_KERNELS[0]);
    let grad_y = image.convolve::<3, 3>(&SOBEL_KERNELS[1]);

    let grad_mag = std::iter::zip(&grad_x.data, &grad_y.data)
        .map(|(x, y)| {
            (x.powi(2) + y.powi(2)).sqrt().min(1023.0)
        })
        .collect::<Vec<_>>();

    let grad_dir = std::iter::zip(&grad_x.data, &grad_y.data)
        .map(|(x, y)| {
            y.atan2(*x) * 180.0 / PI
        })
        .collect::<Vec<_>>();

    (
        Image {
            width: image.width,
            height: image.height,
            data: grad_mag,
        },
        Image{
            width: image.width,
            height: image.height,
            data: grad_dir,
        }
    )
}

/// Future Optimization: Take a mutable grad_dir to skip allocation of a new image. Allows for
/// in-place modification. We only need the direction at the top of the inner loop for theta, after
/// which it's not used.
#[instrument(skip(grad_mag, grad_dir))]
pub fn non_maximum_suppression(grad_mag: &Image, grad_dir: &Image) -> Image {
    let mut suppressed = Image {
        width: grad_mag.width,
        height: grad_mag.height,
        data: vec![0.0; grad_mag.data.len()],
    };

    fn data_padded(image: &Image, x: isize, y: isize) -> f32 {
        if x < 0 || y < 0 {
            0.0
        } else if x as usize >= image.width || y as usize >= image.height {
            0.0
        } else {
            let idx = y as usize * image.width + x as usize;
            image.data[idx]
        }
    }

    for y in 0..suppressed.height {
        for x in 0..suppressed.width {
            let idx = y * suppressed.width + x;
            let mag = grad_mag.data[idx];
            let mut theta = grad_dir.data[idx];

            let xi = x as isize;
            let yi = y as isize;

            if theta < 0.0 {
                theta += 180.0;
            }

            let theta = (theta / 45.0).round() * 45.0;

            // directional neighbors
            let (q, r) = match theta {
                0.0 | 180.0 => {
                    (data_padded(grad_mag, xi - 1, yi), data_padded(grad_mag, xi + 1, yi))
                }
                45.0 => {
                    (data_padded(grad_mag, xi - 1, yi + 1), data_padded(grad_mag, xi + 1, yi - 1))
                }
                90.0 => {
                    (data_padded(grad_mag, xi, yi - 1), data_padded(grad_mag, xi, yi - 1))
                }
                135.0 => {
                    (data_padded(grad_mag, xi - 1, yi - 1), data_padded(grad_mag, xi + 1, yi + 1))
                }
                _ => (0.0, 0.0)
            };

            if mag >= q && mag >= r {
                suppressed.data[idx] = mag
            } else {
                suppressed.data[idx] = 0.0;
            }
        }
    }

    suppressed
}

fn edge_tracking_by_hysteresis(image: &Image) -> Image {
    let mut edges = Image {
        width: image.width,
        height: image.height,
        data: image.data.clone(),
    };

    for y in 0..image.height {
        for x in 0..image.width {
            let idx = y * image.width + x;
            // weak edge
            if image.data[idx] == 300.0 {
                if has_strong_neighbor(image, x as isize, y as isize) {
                    // ape together strong
                    edges.data[idx] = 1023.0;
                } else {
                    edges.data[idx] = 0.0;
                }
            }
        }
    }

    edges
}

fn has_strong_neighbor(image: &Image, x: isize, y: isize) -> bool {
    for i in -1..=1 {
        for j in -1..=1 {
            if !(i == 0 && j == 0) {
                let nx = x + i;
                let ny = y + j;

                if nx >= 0 && nx < image.width as isize && ny >= 0 && ny < image.height as isize {
                    let idx = (ny as usize) * image.width + (nx as usize);
                    if image.data[idx] == 1023.0 {
                        return true;
                    }
                }
            }
        }
    }
    false
}
