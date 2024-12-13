use std::{fs::File, io::{BufReader, BufWriter, Read}, path::{Path, PathBuf}, time::Instant};
use clap::Parser;
use orb_isp::{edge, image::{Image, RGBImage}, isp::{self, debayer, debayer_old}, utils};
use orb_isp::gaussian;
use tracing::{info, debug, warn, Level};
use tracing_flame::FlameLayer;
use tracing_subscriber::{FmtSubscriber, Registry, prelude::*};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[clap(disable_help_flag = true)]
struct Args {
    /// Width of the image
    #[arg(short, long, default_value_t = 3280)]
    width: usize,

    /// Height of the image
    #[arg(short, long, default_value_t = 2464)]
    height: usize,

    /// Input RAW file path
    #[arg(short, long)]
    file: String,

    #[clap(long, action = clap::ArgAction::HelpLong)]
    help: Option<bool>,
}

fn setup_global_collector(dir: &Path) -> impl Drop {
    let fmt_layer = tracing_subscriber::fmt::Layer::default()
        .with_file(true)
        .with_line_number(true)
        .with_timer(tracing_subscriber::fmt::time::LocalTime::rfc_3339())
        .pretty();

    let (flame_layer, _guard) = FlameLayer::with_file(dir.join("tracing.folded")).unwrap();

    let collector = Registry::default().with(flame_layer).with(fmt_layer);

    tracing::subscriber::set_global_default(collector).unwrap();

    _guard
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let start = Instant::now();

    let filename = PathBuf::from(args.file);
    info!("Processing input file: {}", filename.display());
    
    let mut file = File::open(&filename)?;
    let dir = filename.with_extension("d");
    
    if let Err(e) = std::fs::create_dir(&dir) {
        if e.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(e.into());
        }
    }

    // Initialize tracing subscriber
    let _guard = setup_global_collector(&dir);

    info!("Starting ISP pipeline");
    
    let mut raw_data = Vec::new();
    file.read_to_end(&mut raw_data)?;

    let expected_size = args.width * args.height;
    if raw_data.len() < expected_size {
        warn!("File size {} bytes is smaller than expected {} bytes ({} x {})", 
            raw_data.len(), expected_size, args.width, args.height);
        return Err(format!("Input file is too small: expected at least {} bytes", expected_size).into());
    }
    debug!("Read {} bytes of raw data", raw_data.len());

    let raw_data = utils::process_jetson_raw10(&raw_data, args.width);

    let image = Image::new(args.width, args.height, raw_data.into_iter().map(From::from).collect::<Vec<_>>());
    image.save_tga(&dir.join("base_rgb.tga"));

    let debayered_image = debayer::debayer(&image, dir.clone());

    let mut grayscale = debayered_image.to_grayscale();
    grayscale.save_gray_tga(&dir.join("debayered_3x3_bilinear_gray.tga"));
    grayscale.normalize_mut();


    let thresholds: Vec<(f32, f32)> = vec![
        (1.25, 10.0),
        (2.5, 20.0),
        (2.5, 15.0),
        (10.0, 20.0),
        (10.0, 40.0),
        (30.0, 150.0),
    ];

    for threshold in thresholds.iter() {
        let (low, high) = threshold;
        let low = (low / 255.0) * 1023.0;
        let high = (high / 255.0) * 1023.0;
        edge::canny2::canny_edge_detection(&image, low, high, &dir);
    }


    //let old_debayered = debayer_old::debayer(&image);
    //old_debayered.save_tga(&dir.join("debayered_3x3_rgb"));

    //let debayered_image = debayer::debayer(&image, dir.clone());
    //debayered_image.save_tga(&dir.join("debayered_3x3_bilinear_rgb.tga"));

    //let mut grayscale = debayered_image.to_grayscale();
    //grayscale.save_gray_tga(&dir.join("debayered_3x3_bilinear_gray.tga"));
    //grayscale.find_bad_vals();

    ////edge::sobel_edge_detection(&grayscale, &dir.join("gray_alt_edge_sobel.tga"));

    //grayscale.normalize_mut();
    //grayscale.save_gray_tga(&dir.join("gray_normalized.tga"));

    //gaussian::blur_mut(&mut grayscale);
    //grayscale.save_gray_tga(&dir.join("gray_blurred.tga"));

    //let mut edge = edge::sobel(&grayscale);
    //edge.save_gray_tga(&dir.join("gray_edge_sobel.tga"));
    //edge::threshold(&mut edge, 110.0);
    //edge.save_gray_tga(&dir.join("gray_edge_sobel_thresh.tga"));

    //{
    //    let components = edge::connected_components(&edge);
    //    paint_connected_components(&components, edge.width, edge.height).save_tga(&dir.join("connected_rgb.tga"));

    //    let contours = edge::extract_contours(&components, edge.width, edge.height);
    //    let mut polygons = Vec::new();
    //    for (_, contour) in &contours {
    //        let polygon = edge::approximate_polygon_dp(contour, 10.0);
    //        polygons.push(polygon);
    //    }
    //    info!("adjust for {} polygons", polygons.len());

    //    let mut squares = Vec::new();
    //    for polygon in polygons {
    //        if edge::is_square(&polygon) {
    //            squares.push(polygon.clone())
    //        }
    //    }

    //    info!("found {} squares", squares.len());
    //}
        
    let duration = start.elapsed();
    info!("ISP pipeline completed in {:.2?}", duration);
    drop(_guard);
    make_flamegraph(&dir, &dir.join("tracing.svg"));
    Ok(())
}

pub fn paint_connected_components(labels: &Vec<usize>, width: usize, height: usize) -> RGBImage {
    let mut rgb_image = RGBImage {
        width,
        height,
        data: vec![[0.0; 3]; width * height],
    };

    let mut counter = 0;
    let mut max = 0;

    for (idx, &label) in labels.iter().enumerate() {
        if label == 0 {
            continue;
        } else {
            counter+=1;
            if label > max {
                max = label;
            }
        }
        
        // generate a hue value between 0 and 360 degrees based on the label
        // - https://medium.com/@winwardo/simple-non-repeating-colour-generation-6efc995832b8
        // 
        // This is a very neat trick.
        let hue = (label as f32 * 137.0) % 360.0;
        let saturation = 0.95;
        let value = 0.95;

        // HSV -> RGB
        let h = hue / 60.0;
        let i = h.floor();
        let f = h - i;
        let p = value * (1.0 - saturation);
        let q = value * (1.0 - saturation * f);
        let t = value * (1.0 - saturation * (1.0 - f));

        let (r, g, b) = match i as i32 {
            0 => (value, t, p),
            1 => (q, value, p),
            2 => (p, value, t),
            3 => (p, q, value),
            4 => (t, p, value),
            _ => (value, p, q),
        };

        rgb_image.data[idx] = [r * 1023.0, g * 1023.0, b * 1023.0];
    }
    debug!(non_background = counter, max_label = max, "non-background pixels found");

    rgb_image
}

fn make_flamegraph(dir: &Path, out: &Path) {
    println!("outputting flamegraph to {}", out.display());
    let inf = File::open(dir.join("tracing.folded")).unwrap();
    let reader = BufReader::new(inf);

    let out = File::create(out).unwrap();
    let writer = BufWriter::new(out);

    let mut opts = inferno::flamegraph::Options::default();
    inferno::flamegraph::from_reader(&mut opts, reader, writer).unwrap();
}
