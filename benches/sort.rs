use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use sorting_benchmarks::{Distribution, par_radix_sort_i32_11bit, par_sort_unstable};
use std::hint::black_box;
use std::time::Duration;

const SIZES: &[usize] = &[1_00, 100_000, 1_000_000, 10_000_000];

fn sorting_benchmarks(c: &mut Criterion) {
    for distribution in Distribution::ALL {
        for &size in SIZES {
            let mut group = c.benchmark_group(format!("sort/{}/{}", distribution.name(), size));
            let input = distribution.values(size);
            group.throughput(Throughput::Elements(size as u64));

            bench_sort(&mut group, "par_sort_unstable", &input, par_sort_unstable);
            bench_sort(
                &mut group,
                "par_radix_sort_i32_11bit",
                &input,
                par_radix_sort_i32_11bit,
            );
            // bench_sort(
            //     &mut group,
            //     "par_radix_sort_i32_8bit2",
            //     &input,
            //     par_radix_sort_i32_8bit2,
            // );

            group.finish();
        }
    }
}

fn bench_sort(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    input: &[i32],
    sort: fn(&mut [i32]),
) {
    group.bench_with_input(BenchmarkId::from_parameter(name), input, |b, input| {
        b.iter_batched(
            || input.to_vec(),
            |mut values| {
                sort(&mut values);
                black_box(values);
            },
            criterion::BatchSize::LargeInput,
        );
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .without_plots()
        .warm_up_time(Duration::from_millis(1000))
        .measurement_time(Duration::from_secs(2))
        .sample_size(20);
    targets = sorting_benchmarks
}
criterion_main!(benches);
