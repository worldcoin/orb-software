use crate::image::Image;

pub fn dpc_median_mut(image: &mut Image) {
    let max_value = 1.0;
    let dead_thres = 0.2;

    for y in (2..image.height - 2) {
        for x in (2..image.width - 2) {
            let mut is_dead = false;

            let idx = y * image.width + x;
            let current = image.data[idx];
            let mut neighbors = [
                image.data[idx - (image.width * 2) - 2],
                image.data[idx - (image.width * 2)],
                image.data[idx - (image.width * 2) + 2],
                image.data[idx - 2],
                image.data[idx + 2],
                image.data[idx + (image.width * 2) - 2],
                image.data[idx + (image.width * 2)],
                image.data[idx + (image.width * 2) + 2],
            ];

            neighbors.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let min = f32::max(dead_thres, neighbors[0]);
            let max = f32::min(max_value - dead_thres, neighbors[7]);
            neighbors[0] = min;
            neighbors[7] = max;

            if current < neighbors[0] - dead_thres || current > neighbors[7] + dead_thres {
                is_dead = true;
            }

            if is_dead {
                image.data[idx] = (neighbors[3] + neighbors[4]) / 2.0;
            }
        }
    }
}

pub fn dpc_median(image: &Image) -> Image {
    let mut image = image.clone();
    dpc_median_mut(&mut image);
    image
}
