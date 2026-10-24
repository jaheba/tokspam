use std::time::{Duration, Instant};

pub fn median_ns(mut run: impl FnMut()) -> f64 {
    for _ in 0..3 {
        run();
    }
    let mut iterations = 1usize;
    loop {
        let start = Instant::now();
        for _ in 0..iterations {
            run();
        }
        if start.elapsed() >= Duration::from_millis(30) || iterations >= 1_000_000 {
            break;
        }
        iterations *= 2;
    }
    let mut samples = [0.0; 7];
    for sample in &mut samples {
        let start = Instant::now();
        for _ in 0..iterations {
            run();
        }
        *sample = start.elapsed().as_nanos() as f64 / iterations as f64;
    }
    samples.sort_by(f64::total_cmp);
    samples[3]
}
