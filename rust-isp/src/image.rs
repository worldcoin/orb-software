use std::path::Path;

use super::utils::tga::{self, ColorSpace};

pub trait Convolvable {
    fn convolve_mut<const M: usize, const N: usize>(&mut self, kernel: &[[f32; N]; N]);
    
    fn convolve<const M: usize, const N: usize>(&self, kernel: &[[f32; N]; N]) -> Self 
    where
        Self: Sized + Clone,
    {
        let mut result = self.clone();
        result.convolve_mut::<M, N>(kernel);
        result
    }

    fn reflect(position: i32, max_size: i32) -> usize {
        if position < 0 {
            (position.abs() - 1) as usize
        } else if position >= max_size {
            (2 * max_size - position - 1) as usize
        } else {
            position as usize
        }
    }
}

#[derive(Clone)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
}

impl Convolvable for Image {
    fn convolve_mut<const M: usize, const N: usize>(&mut self, kernel: &[[f32; N]; N]) {
        let kernel_center_y: i32 = M as i32 / 2;
        let kernel_center_x: i32 = N as i32 / 2;

        let mut convolved = vec![0.0; self.data.len()];

        for y in 0..self.height {
            for x in 0..self.width {
                let mut sum = 0.0;
                for ky in 0..N {
                    for kx in 0..N {
                        let ix = x as i32 + kx as i32 - kernel_center_x;
                        let iy = y as i32 + ky as i32 - kernel_center_y;

                        // Reflect coordinates over image edges
                        let rx = Self::reflect(ix, self.width as i32);
                        let ry = Self::reflect(iy, self.height as i32);

                        let idx = ry * self.width + rx;
                        let pixel = self.data[idx];
                        let weight = kernel[ky][kx];
                        sum += pixel * weight;
                    }
                }
                let idx = y * self.width + x;
                convolved[idx] = sum;
            }
        }

        self.data = convolved;
    }
}

impl Image {
    pub fn new(width: usize, height: usize, data: Vec<f32>) -> Self {
        assert_eq!(data.len(), (width * height) as usize);
        Self { width, height, data }
    }

    pub fn normalize_mut(&mut self) {
        let max_val = self.data.iter().copied().reduce(f32::max).unwrap_or(1023.0);
        self
            .data
            .iter_mut()
            .map(|pixel| {
                *pixel /= max_val;
                *pixel *= 1023.0;
            })
            .collect()
    }

    /// Crop image to specified dimensions. Decreasing the height will remove rows from the
    /// "bottom", decreasing width will remove columns from the "right"
    pub fn crop(&mut self, new_width: usize, new_height: usize) {
        assert!(
            new_width <= self.width,
            "cropped width must be less than or equal to original width"
        );
        assert!(
            new_height <= self.height,
            "cropped height must be less than or equal to original height"
        );

        let mut new_data = Vec::with_capacity(new_width * new_height);

        for row in 0..new_height {
            let start = row * self.width;
            let end = start + new_width;
            new_data.extend_from_slice(&self.data[start..end]);
        }

        self.width = new_width;
        self.height = new_height;
        self.data = new_data;

    }

    pub fn save_tga<P: AsRef<Path>>(&self, path: P) {
        let mut bgr_data = Vec::with_capacity(self.width * self.height * 3);
        for y in 0..self.height {
            for x in 0..self.width {
                //   0 1 0 1
                // 0 R G R G
                // 1 G B G B
                let idx = (y * self.width + x) as usize;
                match (x % 2, y % 2) {
                    (0, 0) => bgr_data.append(&mut vec![0, 0, ((self.data[idx].min(1024.0) / 1024.0) * 255.0) as u8]),
                    (1, 0) => bgr_data.append(&mut vec![0, ((self.data[idx].min(1024.0) / 1024.0) * 255.0) as u8, 0]),
                    (0, 1) => bgr_data.append(&mut vec![0, ((self.data[idx].min(1024.0) / 1024.0) * 255.0) as u8, 0]),
                    (1, 1) => bgr_data.append(&mut vec![((self.data[idx].min(1024.0) / 1024.0) * 255.0) as u8, 0, 0]),
                    _ => unreachable!(),
                }
            }
        }

        tga::write(&bgr_data, self.width as u32, self.height as u32, ColorSpace::BGR, path);
    }

    pub fn find_bad_vals(&self) {
        for y in 0..self.height {
            for x in 0..self.width {
                let idx = (y * self.width + x) as usize;
                let val = self.data[idx];
                if !val.is_normal() && val != 0.0 {
                    panic!("found abnormal value `{val}` at {x},{y}");
                }
            }
        }
    }

    pub fn save_gray_tga<P: AsRef<Path>>(&self, path: P) {
        let gray_data = self.data.iter().map(|val| ((val.min(1024.0) / 1024.0) * 255.0) as u8).collect::<Vec<_>>();
        tga::write(&gray_data, self.width as u32, self.height as u32, ColorSpace::Gray, path);
    }
}

#[derive(Clone)]
pub struct MultiChannelImage<const C: usize> {
    pub width: usize,
    pub height: usize,
    pub data: Vec<[f32; C]>,
}

impl<const C: usize> Convolvable for MultiChannelImage<C> {
    fn convolve_mut<const M: usize, const N: usize>(&mut self, kernel: &[[f32; N]; N]) {
        let kernel_center_y: i32 = M as i32 / 2;
        let kernel_center_x: i32 = N as i32 / 2;

        let mut convolved = vec![[0.0; C]; self.data.len()];

        for y in 0..self.height {
            for x in 0..self.width {
                let mut sum = [0.0; C];
                for ky in 0..N {
                    for kx in 0..N {
                        let weight = kernel[ky][kx];

                        let ix = x as i32 + kx as i32 - kernel_center_x;
                        let iy = y as i32 + ky as i32 - kernel_center_y;

                        // Reflect coordinates over image edges
                        let rx = Self::reflect(ix, self.width as i32);
                        let ry = Self::reflect(iy, self.height as i32);

                        let idx = ry * self.width + rx;
                        let pixels = self.data[idx];

                        for c in 0..C {
                            if !f32::is_normal(pixels[c]) {
                                panic!("found non normal value at {y}, {x}");
                            }
                            sum[c] += pixels[c] * weight;
                        }
                    }
                }

                let idx = y * self.width + x;
                convolved[idx] = sum;
            }
        }

        self.data = convolved;
    }
}

impl<const C: usize> MultiChannelImage<C> {
    pub fn from_channels<const M: usize, const N: usize>(channels: &[Image], mapping: &[[[usize; N]; M]; C]) -> Self {
        let width = channels[0].width;
        let height = channels[0].height;
        let mut data = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                let mapping_x = x % N;
                let mapping_y = y % M;

                let mut value = [0.0; C];

                // Should look into rewriting this. I forgot about the double-green
                // channels when first implementing, which means there's not a clean
                // mapping from input channels to output channels as we separate
                // out G/B from G/R :(
                for c in 0..C {
                    let mapping_idx = mapping[c][mapping_y][mapping_x];
                    let image_idx = y * width + x;
                    value[c] = channels[mapping_idx].data[image_idx];
                }
                data.push(value);
            }
        }

        Self { width, height, data }
    }

    
}

pub type RGBImage = MultiChannelImage<3>;

impl RGBImage {
    pub fn save_tga<P: AsRef<Path>>(&self, path: P) {
        let mut data = Vec::with_capacity(self.width * self.height * 3);
        for pixel in self.data.iter() {
            // Save as normalized 8-bit BGR
            data.push(((pixel[2].min(1024.0) / 1024.0) * 255.0) as u8);
            data.push(((pixel[1].min(1024.0) / 1024.0) * 255.0) as u8);
            data.push(((pixel[0].min(1024.0) / 1024.0) * 255.0) as u8);
        }

        tga::write(&data, self.width as u32, self.height as u32, ColorSpace::BGR, path);
    }

    pub fn to_grayscale(&self) -> Image {
        let mut data = Vec::with_capacity(self.width * self.height);
        for pixel in self.data.iter() {
            // Convert RGB to grayscale using standard coefficients
            // https://cadik.posvete.cz/color_to_gray_evaluation/
            let gray = 0.299 * pixel[0] + 0.587 * pixel[1] + 0.114 * pixel[2];
            data.push(gray);
        }

        Image::new(self.width, self.height, data)
    }
}

pub enum RGBChannel {
    Red,
    Green,
    Blue,
}
pub fn save_single_channel<P: AsRef<Path>>(image: &Image, path: P, channel: RGBChannel) {
    let mut bgr_data = Vec::with_capacity(image.width * image.height * 3);            
    for pixel in image.data.iter() {                                                      
        let value = ((pixel.min(1024.0) / 1024.0) * 255.0) as u8;                         
        match channel {
            RGBChannel::Red => bgr_data.extend_from_slice(&[0, 0, value]),
            RGBChannel::Green => bgr_data.extend_from_slice(&[0, value, 0]),
            RGBChannel::Blue => bgr_data.extend_from_slice(&[value, 0, 0])
        }
    }                                                                                     
                                                                                          
    tga::write(&bgr_data, image.width as u32, image.height as u32, ColorSpace::BGR, path);
}
