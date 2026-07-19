use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use sorting_benchmarks::msd::sort::{
    BUCKETS_4, BUCKETS_7, BUCKETS_8, i32_first_shift, par_msd_radix_sort_in,
};
use sorting_benchmarks::{
    Distribution, Radix11Scratch, par_radix_sort_i32_11bit_with_scratch, par_sort_unstable,
};
use std::hint::black_box;
use std::time::Duration;

const SIZES: &[usize] = &[
    10_000, 25_000, 50_000, 75_000, 100_000, 150_000, 250_000, 500_000, 750_000,
    1_000_000,
    // 2_500_000,
    // 5_000_000,
    // 10_000_000,
    // 100_000_000,
    // 1_000_000_000,
];

fn radix_vs_quicksort(c: &mut Criterion) {
    let distribution = Distribution::Random;
    let mut group = c.benchmark_group(format!("radix_vs_quicksort/{}", distribution.name()));

    for &size in SIZES {
        let input = distribution.values(size);
        group.throughput(Throughput::Elements(size as u64));

        group.bench_with_input(
            BenchmarkId::new("par_sort_unstable", size),
            &input,
            |b, input| {
                b.iter_batched(
                    || input.to_vec(),
                    |mut values| {
                        par_sort_unstable(&mut values);
                        black_box(values);
                    },
                    criterion::BatchSize::LargeInput,
                );
            },
        );

        // group.bench_with_input(
        //     BenchmarkId::new("par_sort_unstable_new", size),
        //     &input,
        //     |b, input| {
        //         b.iter_batched(
        //             || input.to_vec(),
        //             |mut values| {
        //                 let sorted = par_sort_unstable_new(&mut values);
        //                 black_box(sorted);
        //             },
        //             criterion::BatchSize::LargeInput,
        //         );
        //     },
        // );

        group.bench_with_input(
            BenchmarkId::new("par_radix_sort_i32_11bit_scratch", size),
            &input,
            |b, input| {
                let mut scratch = Radix11Scratch::new();
                let mut warmup = input.to_vec();
                par_radix_sort_i32_11bit_with_scratch(&mut warmup, &mut scratch);
                b.iter_batched(
                    || input.to_vec(),
                    |mut values| {
                        par_radix_sort_i32_11bit_with_scratch(&mut values, &mut scratch);
                        black_box(values);
                    },
                    criterion::BatchSize::LargeInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("par_msd_8bit", size),
            &input,
            |b, input| {
                let mut dst = vec![0i32; input.len()];
                b.iter_batched(
                    || input.to_vec(),
                    |mut values| {
                        par_msd_radix_sort_in::<BUCKETS_8, BUCKETS_8>(
                            &mut values,
                            &mut dst,
                            i32_first_shift::<BUCKETS_8>(),
                        );
                        black_box(values);
                    },
                    criterion::BatchSize::LargeInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("par_msd_7bit", size),
            &input,
            |b, input| {
                let mut dst = vec![0i32; input.len()];
                b.iter_batched(
                    || input.to_vec(),
                    |mut values| {
                        par_msd_radix_sort_in::<BUCKETS_7, BUCKETS_4>(
                            &mut values,
                            &mut dst,
                            i32_first_shift::<BUCKETS_7>(),
                        );
                        black_box(values);
                    },
                    criterion::BatchSize::LargeInput,
                );
            },
        );

        // group.bench_with_input(
        //     BenchmarkId::new("par_msd_6bit", size),
        //     &input,
        //     |b, input| {
        //         let mut dst = vec![0i32; input.len()];
        //         b.iter_batched(
        //             || input.to_vec(),
        //             |mut values| {
        //                 par_msd_radix_sort_in::<BUCKETS_6, BUCKETS_2>(
        //                     &mut values,
        //                     &mut dst,
        //                     i32_first_shift::<BUCKETS_6>(),
        //                 );
        //                 black_box(values);
        //             },
        //             criterion::BatchSize::LargeInput,
        //         );
        //     },
        // );

        // group.bench_with_input(
        //     BenchmarkId::new("par_msd_5bit", size),
        //     &input,
        //     |b, input| {
        //         let mut dst = vec![0i32; input.len()];
        //         b.iter_batched(
        //             || input.to_vec(),
        //             |mut values| {
        //                 par_msd_radix_sort_in::<BUCKETS_5, BUCKETS_2>(
        //                     &mut values,
        //                     &mut dst,
        //                     i32_first_shift::<BUCKETS_5>(),
        //                 );
        //                 black_box(values);
        //             },
        //             criterion::BatchSize::LargeInput,
        //         );
        //     },
        // );

        //     group.bench_with_input(BenchmarkId::new("par_msd_new", size), &input, |b, input| {
        //         b.iter_batched(
        //             || input.to_vec(),
        //             |mut values| {
        //                 let sorted = par_msd_radix_sort_8bit_new(&mut values);
        //                 black_box(sorted);
        //             },
        //             criterion::BatchSize::LargeInput,
        //         );
        //     });
        // }
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(1))
        .sample_size(10);
    targets = radix_vs_quicksort
}
criterion_main!(benches);
