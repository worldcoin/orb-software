use std::path::PathBuf;

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use orb_isp::isp::{awb, debayer::debayer, image::Image};
use rand::{rngs::StdRng, Rng, SeedableRng};

fn generate_random_image(width: usize, height: usize, max_value: usize) -> Image {
    let seed = [0u8; 32];
    let mut rng = StdRng::from_seed(seed);

    let data: Vec<f32> = (0..width * height)
        .map(|_| rng.gen_range(0..max_value) as f32)
        .collect();

    Image { width, height, data }
}

fn benchmark_awb(c: &mut Criterion) {
    const WIDTH: usize = 3280;
    const HEIGHT: usize = 2464;
    const MAX_VALUE: usize = 1 << 10; // 1024 for 10-bit images

    // Generate the random image data once
    let image = generate_random_image(WIDTH, HEIGHT, MAX_VALUE);

    let mut group = c.benchmark_group("AWB");

    group.bench_with_input(BenchmarkId::new("AWB", WIDTH * HEIGHT), &image, |b, img| {
        b.iter(|| {
            let _result = awb::awb(black_box(img));
        })
    });

    group.finish();
}

fn benchmark_debayer(c: &mut Criterion) {
    const WIDTH: usize = 3280;
    const HEIGHT: usize = 2464;
    const MAX_VALUE: usize = 1 << 10; // 1024 for 10-bit images

    // Generate the random image data once
    let image = generate_random_image(WIDTH, HEIGHT, MAX_VALUE);

    let mut group = c.benchmark_group("Debayer");
    
    group.bench_with_input(BenchmarkId::new("Debayer", WIDTH * HEIGHT), &image, |b, img| {
        b.iter(|| {
            let _result = debayer(black_box(img), PathBuf::from("/"));
        })
    });

    group.finish();

}

fn benchmark_dpc(c: &mut Criterion) {

}

criterion_group!(benches, benchmark_awb, benchmark_debayer);
criterion_main!(benches);
