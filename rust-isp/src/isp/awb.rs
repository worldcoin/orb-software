use tracing::instrument;

use crate::image::Image;

#[instrument(skip(image))]
pub fn awb(image: &Image) -> Image {
    let (width, height, data) = (image.width, image.height, &image.data);
    const BIT_DEPTH: usize = 10;
    const MAX_VALUE: usize = 1 << BIT_DEPTH;

    let mut r_hist = vec![0usize; MAX_VALUE];
    let mut g_hist = vec![0usize; MAX_VALUE];
    let mut b_hist = vec![0usize; MAX_VALUE];
    // track the pixel count because pre-debayering it's not as simple as len / 3
    let (mut total_r, mut total_g, mut total_b) = (0usize, 0usize, 0usize);

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let pixel = data[idx] as usize;
            if pixel >= MAX_VALUE {
                continue;
            }
            match (y % 2, x % 2) {
                // R pixel
                (0, 0) => {
                    r_hist[pixel] += 1;
                    total_r += 1;
                }
                // B pixel
                (1, 1) => {
                    b_hist[pixel] += 1;
                    total_b += 1;
                }
                // G pixel
                _ => {
                    g_hist[pixel] += 1;
                    total_g += 1;
                }
            }
        }
    }

    let r_cdf = cumulative_sum(&r_hist);
    let g_cdf = cumulative_sum(&g_hist);
    let b_cdf = cumulative_sum(&b_hist);

    let (r_low, r_high) = find_thresholds(&r_cdf, total_r);
    let (g_low, g_high) = find_thresholds(&g_cdf, total_g);
    let (b_low, b_high) = find_thresholds(&b_cdf, total_b);

    let a_min = *[r_low, g_low, b_low].iter().min().unwrap();
    let a_max = *[r_high, g_high, b_high].iter().max().unwrap();

    // precompute transformation maps
    let r_map = compute_transform_map(r_low, r_high, a_min, a_max, MAX_VALUE);
    let g_map = compute_transform_map(g_low, g_high, a_min, a_max, MAX_VALUE);
    let b_map = compute_transform_map(b_low, b_high, a_min, a_max, MAX_VALUE);

    let mut result_data = Vec::with_capacity(data.len());
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let pixel = data[idx] as usize;
            if pixel >= MAX_VALUE {
                result_data.push(0.0);
            }
            let val = match (y % 2, x % 2) {
                (0, 0) => r_map[pixel],
                (1, 1) => b_map[pixel],
                _ => g_map[pixel],
            };
            result_data.push(val);
        }
    }

    Image {
        width,
        height,
        data: result_data,
    }
}

#[instrument(skip(hist))]
fn cumulative_sum(hist: &[usize]) -> Vec<usize> {
    hist.iter()
        .scan(0usize, |sum, &count| {
            *sum += count;
            Some(*sum)
        })
        .collect()
}

#[instrument(skip(cdf, total_pixels))]
fn find_thresholds(cdf: &[usize], total_pixels: usize) -> (usize, usize) {
    let low = cdf
        .iter()
        .position(|&v| v as f64 >= total_pixels as f64 * 0.01)
        .unwrap_or(0);
    let high = cdf
        .iter()
        .position(|&v| v as f64 >= total_pixels as f64 * 0.99)
        .unwrap_or(cdf.len() - 1);
    (low, high)
}

#[instrument(skip(low, high, a_min, a_max, max_value))]
fn compute_transform_map(
    low: usize,
    high: usize,
    a_min: usize,
    a_max: usize,
    max_value: usize,
) -> Vec<f32> {
    let mut map = vec![0.0; max_value];
    if high > low {
        let scale = (a_max - a_min) as f32 / (high - low) as f32;
        for i in 0..max_value {
            let val = ((i as isize - low as isize) as f32 * scale + a_min as f32).round();
            map[i] = val.clamp(0.0, (max_value - 1) as f32);
        }
    } else {
        // identity
        for i in 0..max_value {
            map[i] = i as f32;
        }
    }
    map
}
