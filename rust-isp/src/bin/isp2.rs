use std::{fs::File, io::{BufReader, BufWriter, Read}, path::{Path, PathBuf}, time::Instant};
use clap::Parser;
use orb_isp::{image::Image, isp::{awb, debayer}, utils};
use tracing::{info, debug, warn};
use tracing_flame::FlameLayer;
use tracing_subscriber::{Registry, prelude::*};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[clap(disable_help_flag = true)]
struct Args {
    #[arg(short, long, default_value_t = 3280)]
    width: usize,
    #[arg(short, long, default_value_t = 2464)]
    height: usize,

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

    
    let mut raw_data = Vec::new();
    file.read_to_end(&mut raw_data)?;

    let expected_size = args.width * args.height;
    if raw_data.len() < expected_size {
        warn!("File size {} bytes is smaller than expected {} bytes ({} x {})", 
            raw_data.len(), expected_size, args.width, args.height);
        return Err(format!("Input file is too small: expected at least {} bytes", expected_size).into());
    }
    debug!("Read {} bytes of raw data", raw_data.len());

    info!("Starting ISP pipeline");
    let start = Instant::now();

    let raw_data = utils::process_jetson_raw10(&raw_data, args.width);

    let mut image = Image::new(args.width, args.height, raw_data.into_iter().map(From::from).collect::<Vec<_>>());
    //image.save_tga(&dir.join("base_rgb_test.tga"));

    // Get rid of the garbage on the right side
    image.crop(image.width - 32, image.height);
    //image.save_tga(&dir.join("base_rgb.tga"));

    let image = awb::awb(&image);

    let debayered_image = debayer::debayer(&image, dir.clone());
    debayered_image.save_tga(&dir.join("debayered_3x3_bilinear_rgb.tga"));

    let duration = start.elapsed();
    info!("ISP pipeline completed in {:.2?}", duration);

    drop(_guard);
    make_flamegraph(&dir, &dir.join("tracing.svg"));
    Ok(())
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
