// TODO: Generalize this for different bits-per-pixel
// TODO: Document Jetson-specific MSB padding (see Xavier TRM)
pub fn process_jetson_raw10(data: &[u8], width: usize) -> Vec<u16> {
    let mut unpacked = Vec::with_capacity(data.len() / 2);
    let mut i = 0;

    // This is because the actual padding is calculated out based off the bits-per-pixel
    // but we're directly assuming it to be 16 so we can divide by 2
    let padding = width % 32;
    
    while i < unpacked.capacity() {
        // Check if we're at the start of an even row
        let row = i / width;
        let is_even_row = row % 2 == 0;
        let col = i % width;

        // Skip the first PADDING values
        if is_even_row && col < padding {
            // Skip first PADDING values on even rows
            i += padding;
            continue;
        }
        
        let mut val = u16::from_le_bytes([data[2 * i], data[2 * i + 1]]);
        if i < 4 {
            print!("{val:#018b} ->");
        }
        val >>= 5;
        if i < 4 {
            println!("{val:#018b}");
        }
        unpacked.push(val);
        
        // If we're at the end of an even row, add PADDING zeros
        if is_even_row && col == width - padding - 1 {
            for _ in 0..padding {
                unpacked.push(0);
            }
        }
        
        i += 1;
    }
    unpacked
}

pub mod tga {
    use std::io::Write;
    use std::{fs::File, path::Path};

    pub enum ColorSpace {
        BGR,
        Gray
    }

    pub fn write<P: AsRef<Path>>(data: &[u8], width: u32, height: u32, color: ColorSpace, path: P) {
        let mut tga_header = vec![0; 18];            
        // Data Type Code
        // 2 = Uncompressed BGR
        // 3 = Uncompressed B/W
        tga_header[2] = match color {
            ColorSpace::BGR => 2,
            ColorSpace::Gray => 3
        };
        tga_header[12] = (255 & width) as u8;
        tga_header[13] = (255 & (width >> 8)) as u8;
        tga_header[14] = (255 & height) as u8;
        tga_header[15] = (255 & (height >> 8)) as u8;
        // Bits-per-pixel
        tga_header[16] = match color {
            ColorSpace::BGR => 24,
            ColorSpace::Gray => 8
        };
        // Image Descriptor
        // Bit 5 = 0: Origin at lower left-hand corner, 1: Origin at upper left-hand corner
        tga_header[17] = 32;

        let mut file = File::create(path).unwrap();
        let _ = file.write_all(&tga_header);
        let _ = file.write_all(data);
    }
}
