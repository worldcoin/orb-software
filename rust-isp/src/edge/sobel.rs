use std::collections::{HashMap, HashSet};

use tracing::{debug, info, instrument};

use crate::image::{Convolvable, Image};

pub const KERNELS: [[[f32; 3]; 3]; 2] = [
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

/// Calculate the sobel operator magnitude across the image
///
/// Note: we skip any direction computation to speed up compute for the naive usage of the sobel
/// operators. See the canny edge detection for calculation and usage of the edge direction
#[instrument(skip(image), level = "debug")]
pub fn sobel(image: &Image) -> Image {
    let mut passes = Vec::with_capacity(KERNELS.len());
    for kernel in KERNELS.iter() {
        let convolved = image.convolve::<3, 3>(kernel);
        passes.push(convolved);
    }

    let mut counter = 0;
    let data = std::iter::zip(&passes[0].data, &passes[1].data)
        .map(|(x, y)| {
            let value = (x.powi(2) + y.powi(2)).sqrt().min(1023.0);
            if value == 1023.0 { counter+=1 }
            value
        })
        .collect();

    info!(max_values = counter, "completed sobel edge detection");

    Image {
        height: image.height,
        width: image.width,
        data
    }
}

// I don't actually know if this works. I may have messed up, but DFS
// is so brutal here that I don't think I did, I think it's just actually
// that slow. Skipping straight to
// [the Two-pass algorithm](https://en.wikipedia.org/wiki/Connected-component_labeling#Two-pass)
#[instrument(skip(image), level = "debug")]
pub fn connected_components_dfs(image: &Image) -> Vec<usize> {
    let mut labels = vec![0; image.width * image.height];
    let mut current_label = 1;
    let mut stack = Vec::new();
    let start_time = std::time::Instant::now();
    let mut last_log = start_time;

    for y in 0..image.height {
        for x in 0..image.width {
            let idx = (y * image.width) + x;
            if image.data[idx] == 1023.0 {
                labels[idx] = current_label;
                stack.push((x, y));

                debug!(label = current_label, "found start of new component");

                // Look for connected pixels
                while let Some((cx, cy)) = stack.pop() {
                    for ny in cy.saturating_sub(1)..=(cy + 1).min(image.height - 1) {
                        for nx in cx.saturating_sub(1)..=(cx + 1).min(image.height - 1) {
                            let idx = (ny * image.height) + nx;
                            if image.data[idx] == 1023.0 {
                                labels[idx] = current_label;
                                stack.push((nx, ny));
                            }
                        }
                    }
                }

                debug!(label = current_label, "found new component");
                current_label+=1;
                
                if last_log.elapsed().as_secs() >= 5 {
                    info!(current_components = current_label, "processing components");
                    last_log = std::time::Instant::now();
                }
            }
        }
    }

    info!(num_components = current_label, "found any connected components");

    labels
}

// Two-pass connected component labeling
#[instrument(skip(image), level = "debug")]
pub fn connected_components(image: &Image) -> Vec<usize> {
    let mut labels = vec![0; image.width * image.height];
    let mut label = 1;
    let mut parent = Vec::new();
    parent.push(0);

    for y in 0..image.height {
        for x in 0..image.width {
            let idx = y * image.width + x;
            let idx_north = idx.checked_sub(image.width).unwrap_or(0);
            let idx_west = idx.checked_sub(1).unwrap_or(0);

            if image.data[idx] == 1023.0 {
                let mut neighbors = Vec::new();

                if y > 0 && image.data[idx_north] == 1023.0 {
                    neighbors.push(labels[idx_north]);
                }

                if x > 0 && image.data[idx_west] == 1023.0 {
                    neighbors.push(labels[idx_west]);
                }

                if neighbors.is_empty() {
                    labels[idx] = label;
                    parent.push(label);
                    label += 1;
                } else {
                    let min_label = *neighbors.iter().min().unwrap();
                    labels[idx] = min_label;

                    for &neighbor_label in &neighbors {
                        union(&mut parent, neighbor_label, min_label);
                    }
                }
            }
        }
    }

    info!(label_count = labels.iter().collect::<HashSet<_>>().len(), "completed first pass");

    // Second pass: Relabel the components
    for y in 0..image.height {
        for x in 0..image.width {
            let idx = y * image.width + x;
            if labels[idx] > 0 {
                labels[idx] = find(&mut parent, labels[idx]);
            }
        }
    }

    let uniq_labels = labels.iter().collect::<HashSet<_>>();
    info!(label_count = uniq_labels.len(), label_uniq = ?uniq_labels, "completed second pass");

    labels
}

// Union-Find 'find' function with path compression
fn find(parent: &mut Vec<usize>, x: usize) -> usize {
    if parent[x] != x {
        parent[x] = find(parent, parent[x]);
    }
    parent[x]
}

// Union-Find 'union' function
fn union(parent: &mut Vec<usize>, x: usize, y: usize) {
    let x_root = find(parent, x);
    let y_root = find(parent, y);
    if x_root != y_root {
        parent[y_root] = x_root;
    }
}

#[instrument(skip(labels), level = "debug")]
pub fn extract_contours(labels: &Vec<usize>, width: usize, height: usize) -> HashMap<usize, Vec<(usize, usize)>> {
    let mut contours: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();

    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let idx = (y * width) + x;
            let label = labels[idx];
            if label != 0 {
                // Look for well-connected cardinal neighbors
                let edges = [
                    labels[(y - 1) * width + x],
                    labels[(y + 1) * width + x],
                    labels[y * width + x - 1],
                    labels[y * width + x + 1],
                ];

                // If no cardinal neighbors are in current label
                // we've found an edge
                if edges[0] != label || edges[1] != label || edges[2] != label ||
                   edges[3] != label {
                    contours.entry(label).or_default().push((x, y));
                }
            }
        }
    }

    contours
}


// Douglas-Peucker contour simplification
pub fn approximate_polygon_dp(
    points: &[(usize, usize)],
    epsilon: f64,
) -> Vec<(usize, usize)> {
    if points.len() < 3 {
        return points.to_vec();
    }

    let points_f: Vec<(f64, f64)> = points
        .iter()
        .map(|&(x, y)| (x as f64, y as f64))
        .collect();

    let mut dmax = 0.0;
    let mut index = 0;

    let (start, end) = (points_f[0], points_f[points_f.len() - 1]);

    for i in 1..points_f.len() - 1 {
        let d = perpendicular_distance(points_f[i], start, end);
        if d > dmax {
            index = i;
            dmax = d;
        }
    }

    let mut result = Vec::new();

    if dmax > epsilon {
        let mut rec_results1 = approximate_polygon_dp(&points[0..=index], epsilon);
        let mut rec_results2 = approximate_polygon_dp(&points[index..], epsilon);

        rec_results1.pop();
        result.append(&mut rec_results1);
        result.append(&mut rec_results2);
    } else {
        result.push(points[0]);
        result.push(points[points.len() - 1]);
    }

    result
}

pub fn perpendicular_distance(
    point: (f64, f64),
    line_start: (f64, f64),
    line_end: (f64, f64),
) -> f64 {
    let (x0, y0) = point;
    let (x1, y1) = line_start;
    let (x2, y2) = line_end;

    let numerator = ((y2 - y1) * x0 - (x2 - x1) * y0 + x2 * y1 - y2 * x1).abs();
    let denominator = ((y2 - y1).powi(2) + (x2 - x1).powi(2)).sqrt();

    numerator / denominator
}

pub fn is_square(polygon: &Vec<(usize, usize)>) -> bool {
    if polygon.len() != 4 {
        return false;
    }

    let points: Vec<(f64, f64)> = polygon
        .iter()
        .map(|&(x, y)| (x as f64, y as f64))
        .collect();

    let mut sides = Vec::new();
    let mut angles = Vec::new();

    for i in 0..4 {
        let (x1, y1) = points[i];
        let (x2, y2) = points[(i + 1) % 4];

        let side = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
        sides.push(side);

        let (dx1, dy1) = (x2 - x1, y2 - y1);
        let (dx2, dy2) = (
            points[(i + 2) % 4].0 - x2,
            points[(i + 2) % 4].1 - y2,
        );

        let dot_product = dx1 * dx2 + dy1 * dy2;
        let magnitude1 = (dx1.powi(2) + dy1.powi(2)).sqrt();
        let magnitude2 = (dx2.powi(2) + dy2.powi(2)).sqrt();
        let angle = (dot_product / (magnitude1 * magnitude2)).acos();
        angles.push(angle);
    }

    let side_mean = sides.iter().sum::<f64>() / sides.len() as f64;
    let side_variance = sides
        .iter()
        .map(|&s| (s - side_mean).powi(2))
        .sum::<f64>()
        / sides.len() as f64;
    if side_variance > 5.0 {
        return false;
    }

    for &angle in &angles {
        if (angle.to_degrees() - 90.0).abs() > 10.0 {
            return false;
        }
    }

    true
}
