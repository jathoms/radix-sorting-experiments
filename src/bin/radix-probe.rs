use sorting_benchmarks::{Distribution, par_radix_sort_i32_11bit};
use std::time::Instant;

fn main() {
    let size = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(1_000_000);
    let repetitions = std::env::args()
        .nth(2)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(20);
    let input = Distribution::Random.values(size);

    for i in 0..repetitions {
        let mut values = input.clone();
        let start = Instant::now();
        par_radix_sort_i32_11bit(&mut values);
        let elapsed = start.elapsed();

        assert!(values.windows(2).all(|pair| pair[0] <= pair[1]));
        println!("{i}\t{:.3} ms", elapsed.as_secs_f64() * 1_000.0);
    }
}
