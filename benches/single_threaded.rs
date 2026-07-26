use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use rand::{SeedableRng, rngs::StdRng, seq::SliceRandom};
#[allow(unused_imports)]
use sorting_benchmarks::msd::aflag::{
    msd_inplace, msd_once_then_inplace, msd_once_then_inplace_eager, par_msd_inplace,
    par_msd_inplace_eager,
};
#[allow(unused_imports)]
use sorting_benchmarks::msd::sort::{BUCKETS_4, BUCKETS_7, i32_first_shift, par_msd_radix_sort_in};
use sorting_benchmarks::{radix_sort_i32, radix_sort_i32_11bit, sort_unstable};
use std::hint::black_box;
use std::sync::Once;
use std::time::Duration;

const SIZES: &[usize] = &[
    10_000, 50_000, 100_000, 250_000, 500_000, 1_000_000, 5_000_000,
];

#[derive(Clone, Copy)]
enum Pattern {
    Random,
    Sorted,
    Reversed,
    FewUnique,
    SameHighByte,
}

impl Pattern {
    const ALL: &[Self] = &[
        Self::Random,
        Self::Sorted,
        Self::Reversed,
        Self::FewUnique,
        Self::SameHighByte,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Random => "random",
            Self::Sorted => "sorted",
            Self::Reversed => "reversed",
            Self::FewUnique => "few_unique",
            Self::SameHighByte => "same_high_byte",
        }
    }

    fn values(self, size: usize) -> Vec<i32> {
        match self {
            Self::Random => shuffled(size, |i| pseudo_random_i32(i as u32)),
            Self::Sorted => (0..size).map(centered_value).collect(),
            Self::Reversed => (0..size).rev().map(centered_value).collect(),
            Self::FewUnique => shuffled(size, |i| (i as i32 % 128) - 64),
            Self::SameHighByte => shuffled(size, |i| pseudo_random_i32(i as u32) & 0x00ff_ffff),
        }
    }
}

fn single_threaded(c: &mut Criterion) {
    init_single_thread_rayon();

    for pattern in Pattern::ALL {
        let mut group = c.benchmark_group(format!("single_threaded/{}", pattern.name()));

        for &size in SIZES {
            let input = pattern.values(size);
            group.throughput(Throughput::Elements(size as u64));

            bench_in_place(&mut group, "sort_unstable", size, &input, sort_unstable);
            bench_in_place(&mut group, "radix_sort_i32", size, &input, radix_sort_i32);
            bench_in_place(
                &mut group,
                "radix_sort_i32_11bit",
                size,
                &input,
                radix_sort_i32_11bit,
            );
            bench_new_output(
                &mut group,
                "msd_once_then_inplace_eager",
                size,
                &input,
                |input| {
                    let mut values = input.to_vec();
                    let mut output = vec![0i32; input.len()];
                    msd_once_then_inplace_eager(&mut values, &mut output);
                    black_box(output);
                },
            );

            // Toggle-friendly: this uses Rayon internally, but the global pool is one thread here.
            // bench_in_place(&mut group, "par_msd_inplace", size, &input, |values| {
            //     par_msd_inplace(values, 24);
            // });
            // bench_in_place(&mut group, "par_msd_inplace_eager", size, &input, |values| {
            //     par_msd_inplace_eager::<BUCKETS_7, BUCKETS_4>(values, i32_first_shift::<BUCKETS_7>());
            // });
        }

        group.finish();
    }
}

fn bench_in_place(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    size: usize,
    input: &[i32],
    sort: fn(&mut [i32]),
) {
    group.bench_with_input(BenchmarkId::new(name, size), input, |b, input| {
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

fn bench_new_output<F>(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    size: usize,
    input: &[i32],
    mut sort: F,
) where
    F: FnMut(&[i32]),
{
    group.bench_with_input(BenchmarkId::new(name, size), input, |b, input| {
        b.iter(|| sort(input));
    });
}

fn init_single_thread_rayon() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build_global();
    });
}

fn centered_value(i: usize) -> i32 {
    (i as i32).wrapping_sub(i32::MAX / 2)
}

fn shuffled<F>(size: usize, mut value: F) -> Vec<i32>
where
    F: FnMut(usize) -> i32,
{
    let mut values = (0..size).map(&mut value).collect::<Vec<_>>();
    let mut rng = StdRng::seed_from_u64(42);
    values.shuffle(&mut rng);
    values
}

fn pseudo_random_i32(i: u32) -> i32 {
    i.wrapping_mul(1_664_525).wrapping_add(1_013_904_223) as i32
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(1))
        .sample_size(10);
    targets = single_threaded
}
criterion_main!(benches);
