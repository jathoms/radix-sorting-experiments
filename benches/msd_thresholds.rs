use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use sorting_benchmarks::Distribution;
use sorting_benchmarks::msd::paradis::paradis_sort_i32_to_output_with_threshold;
use sorting_benchmarks::msd::sort::{
    BUCKETS_4, BUCKETS_7, i32_first_shift, par_msd_radix_sort_in_threshold,
};
use std::hint::black_box;
use std::time::Duration;

const SIZES: &[usize] = &[100_000, 250_000, 500_000, 1_000_000, 5_000_000, 25_000_000];
const THRESHOLDS: &[usize] = &[8_000, 16_000, 25_000, 32_000, 50_000, 75_000, 128_000];
const PARADIS_SIZES: &[usize] = &[
    5_000, 6_000, 8_000, 10_000, 16_000, 25_000, 50_000, 75_000, 100_000, 150_000, 250_000,
    500_000, 1_000_000,
];
const PARADIS_THRESHOLDS: &[usize] = &[
    0, 1_000, 2_000, 4_000, 8_000, 16_000, 25_000, 32_000, 50_000, 75_000, 100_000, 150_000,
    250_000,
];

fn msd_thresholds(c: &mut Criterion) {
    let distribution = Distribution::Random;

    for &size in SIZES {
        let input = distribution.values(size);
        let mut group = c.benchmark_group(format!(
            "msd_thresholds/{}/7bit/{size}",
            distribution.name()
        ));
        group.throughput(Throughput::Elements(size as u64));

        for &threshold in THRESHOLDS {
            bench_msd_thresholds::<BUCKETS_7, BUCKETS_4>(&mut group, threshold, &input);
        }

        group.finish();
    }
}

fn paradis_thresholds(c: &mut Criterion) {
    let distribution = Distribution::Random;

    for &size in PARADIS_SIZES {
        let input = distribution.values(size);
        let mut group =
            c.benchmark_group(format!("paradis_thresholds/{}/{size}", distribution.name()));
        group.throughput(Throughput::Elements(size as u64));

        for &threshold in PARADIS_THRESHOLDS {
            bench_paradis_threshold(&mut group, threshold, &input);
        }

        group.finish();
    }
}

fn bench_msd_thresholds<const BUCKETS: usize, const RESIDUAL_BUCKETS: usize>(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    threshold: usize,
    input: &[i32],
) {
    group.bench_with_input(
        BenchmarkId::from_parameter(format!("t{threshold}")),
        input,
        |b, input| {
            let mut dst = vec![0i32; input.len()];
            b.iter_batched(
                || input.to_vec(),
                |mut values| {
                    par_msd_radix_sort_in_threshold::<BUCKETS, RESIDUAL_BUCKETS>(
                        &mut values,
                        &mut dst,
                        i32_first_shift::<BUCKETS>(),
                        threshold,
                    );
                    black_box((&values, &dst));
                },
                criterion::BatchSize::LargeInput,
            );
        },
    );
}

fn bench_paradis_threshold(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    threshold: usize,
    input: &[i32],
) {
    group.bench_with_input(
        BenchmarkId::from_parameter(format!("t{threshold}")),
        input,
        |b, input| {
            let mut output = vec![0i32; input.len()];
            b.iter(|| {
                paradis_sort_i32_to_output_with_threshold(input, &mut output, threshold);
                black_box(&output);
            });
        },
    );
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(1))
        .sample_size(10);
    targets = msd_thresholds, paradis_thresholds
}
criterion_main!(benches);
