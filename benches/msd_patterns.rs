use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use rand::{SeedableRng, rngs::StdRng, seq::SliceRandom};
#[allow(unused_imports)]
use sorting_benchmarks::msd::aflag::{
    msd_once_then_inplace_eager, msd_once_then_inplace_new, par_msd_inplace_eager,
};
#[allow(unused_imports)]
use sorting_benchmarks::msd::sort::{
    BUCKETS_4, BUCKETS_7, i32_first_shift, par_msd_radix_sort_7bit, par_msd_radix_sort_in,
    par_sort_unstable,
};
use sorting_benchmarks::{Radix11Scratch, par_radix_sort_i32_11bit_with_scratch};
use std::hint::black_box;
use std::time::Duration;

const SIZES: &[usize] = &[
    10_000, 50_000, 100_000, 250_000, 500_000, 1_000_000, 5_000_000,
];

#[derive(Clone, Copy)]
#[allow(dead_code)]
enum Pattern {
    Random,
    Sorted,
    Reversed,
    FewUnique,
    SameHighByte,
}

impl Pattern {
    #[allow(dead_code)]
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

fn msd_patterns(c: &mut Criterion) {
    for pattern in [Pattern::SameHighByte] {
        let mut group = c.benchmark_group(format!("msd_patterns/{}", pattern.name()));

        for &size in SIZES {
            let input = pattern.values(size);
            group.throughput(Throughput::Elements(size as u64));

            bench_to_output(
                &mut group,
                "par_sort_unstable",
                size,
                &input,
                |input, output| {
                    output.copy_from_slice(input);
                    par_sort_unstable(output);
                    black_box(output);
                },
            );
            bench_to_output(&mut group, "par_msd", size, &input, |input, output| {
                output.copy_from_slice(input);
                let mut scratch = vec![0i32; input.len()];
                par_msd_radix_sort_in::<BUCKETS_7, BUCKETS_4>(
                    output,
                    &mut scratch,
                    i32_first_shift::<BUCKETS_7>(),
                );
                black_box((output, &scratch));
            });
            bench_to_output(
                &mut group,
                "par_msd_once_eager",
                size,
                &input,
                |input, output| {
                    let mut values = input.to_vec();
                    msd_once_then_inplace_eager(&mut values, output);
                    black_box(output);
                },
            );
            // bench_lsd_scratch(&mut group, "par_lsd_11bit_scratch", size, &input);
        }

        group.finish();
    }
}

fn bench_to_output<F>(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    size: usize,
    input: &[i32],
    mut sort: F,
) where
    F: FnMut(&[i32], &mut [i32]),
{
    group.bench_with_input(BenchmarkId::new(name, size), input, |b, input| {
        let mut output = vec![0i32; input.len()];
        b.iter(|| sort(input, &mut output));
    });
}

#[allow(dead_code)]
fn bench_lsd_scratch(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    size: usize,
    input: &[i32],
) {
    group.bench_with_input(BenchmarkId::new(name, size), input, |b, input| {
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
        .warm_up_time(Duration::from_millis(2000))
        .measurement_time(Duration::from_secs(5))
        .sample_size(50);
    targets = msd_patterns
}
criterion_main!(benches);
