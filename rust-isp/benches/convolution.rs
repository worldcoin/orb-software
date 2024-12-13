use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use orb_isp::convolution::convolve;
use rand::Rng;

pub fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("convolve 10k", |b| {
        b.iter_batched(|| {
            let mut rng = rand::thread_rng();
            let (u, v): ([f32; 10_000], [f32; 10_000]) = (rng.gen(), rng.gen());
            (u, v)
        }, |(u, v)| {
            convolve(black_box(&u), black_box(&v))
        }, criterion::BatchSize::SmallInput)
    });
}

criterion_group!{
    name = benches;
    config = Criterion::default().measurement_time(Duration::from_secs(30));
    targets = criterion_benchmark
}
criterion_main!(benches);
