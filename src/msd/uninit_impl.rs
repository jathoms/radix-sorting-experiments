use super::sort::{BUCKETS_4, BUCKETS_7, BUCKETS_8, i32_first_shift};
use std::mem::{ManuallyDrop, MaybeUninit};

const DEFAULT_FALLBACK_THRESHOLD: usize = 50_000;

pub fn par_msd_radix_sort_7bit_new_uninit(values: &[i32]) -> Vec<i32> {
    par_msd_radix_sort_new_uninit::<BUCKETS_7, BUCKETS_4>(values, i32_first_shift::<BUCKETS_7>())
}

pub fn par_msd_radix_sort_8bit_new_uninit(values: &[i32]) -> Vec<i32> {
    par_msd_radix_sort_new_uninit::<BUCKETS_8, BUCKETS_8>(values, i32_first_shift::<BUCKETS_8>())
}

pub fn par_msd_radix_sort_new_uninit<const BUCKETS: usize, const RESIDUAL_BUCKETS: usize>(
    values: &[i32],
    shift: usize,
) -> Vec<i32> {
    if values.len() <= 1 {
        return values.to_vec();
    }

    let (mut dst, mut scratch) = rayon::join(
        || {
            let mut dst = uninit_vec(values.len());
            let mut counts = [0usize; BUCKETS];
            partition_from_slice::<BUCKETS>(values, &mut dst, &mut counts, shift);
            dst
        },
        || uninit_vec(values.len()),
    );

    if shift != 0 {
        let mut src_rest = unsafe { assume_init_slice_mut(&mut dst) };
        let mut scratch_rest = scratch.as_mut_slice();
        let mut counts = [0usize; BUCKETS];
        count_buckets::<BUCKETS>(values, &mut counts, shift);
        cumulative_counts(&mut counts);

        rayon::scope(|scope| {
            let mut start = 0;
            for end in counts {
                let bucket_len = end - start;
                let (bucket_src, next_src) = src_rest.split_at_mut(bucket_len);
                let (bucket_dst, next_dst) = scratch_rest.split_at_mut(bucket_len);

                if bucket_len >= 2 {
                    scope.spawn(|_| {
                        sort_initialized_into_uninit::<BUCKETS, RESIDUAL_BUCKETS>(
                            bucket_src,
                            bucket_dst,
                            shift,
                            DEFAULT_FALLBACK_THRESHOLD,
                        );
                    });
                } else if bucket_len == 1 && output_lands_in_scratch::<BUCKETS>(shift) {
                    bucket_dst[0].write(bucket_src[0]);
                }

                src_rest = next_src;
                scratch_rest = next_dst;
                start = end;
            }
        });
    }

    if output_lands_in_scratch::<BUCKETS>(shift) {
        unsafe { assume_init_vec(scratch) }
    } else {
        unsafe { assume_init_vec(dst) }
    }
}

fn sort_initialized_into_uninit<const BUCKETS: usize, const RESIDUAL_BUCKETS: usize>(
    src: &mut [i32],
    dst: &mut [MaybeUninit<i32>],
    shift: usize,
    fallback_threshold: usize,
) {
    let bits = radix_bits::<BUCKETS>();

    if src.len() < fallback_threshold {
        src.sort_unstable();
        if output_lands_in_scratch::<BUCKETS>(shift) {
            copy_into_uninit(src, dst);
        }
        return;
    }

    if shift < bits {
        partition_from_initialized::<RESIDUAL_BUCKETS>(src, dst, 0);
        return;
    }

    let mut counts = [0usize; BUCKETS];
    partition_from_initialized::<BUCKETS>(src, dst, shift - bits);
    count_buckets::<BUCKETS>(unsafe { assume_init_slice(dst) }, &mut counts, shift - bits);
    cumulative_counts(&mut counts);

    rayon::scope(|scope| {
        let mut src_rest = unsafe { assume_init_slice_mut(dst) };
        let mut dst_rest = initialized_as_uninit_mut(src);
        let mut start = 0;

        for end in counts {
            let bucket_len = end - start;
            let (bucket_src, next_src) = src_rest.split_at_mut(bucket_len);
            let (bucket_dst, next_dst) = dst_rest.split_at_mut(bucket_len);

            if bucket_len >= 2 {
                scope.spawn(|_| {
                    sort_initialized_into_uninit::<BUCKETS, RESIDUAL_BUCKETS>(
                        bucket_src,
                        bucket_dst,
                        shift - bits,
                        fallback_threshold,
                    );
                });
            } else if bucket_len == 1 && output_lands_in_scratch::<BUCKETS>(shift - bits) {
                bucket_dst[0].write(bucket_src[0]);
            }

            src_rest = next_src;
            dst_rest = next_dst;
            start = end;
        }
    });
}

fn partition_from_slice<const BUCKETS: usize>(
    src: &[i32],
    dst: &mut [MaybeUninit<i32>],
    counts: &mut [usize; BUCKETS],
    shift: usize,
) {
    count_buckets::<BUCKETS>(src, counts, shift);
    prefix_counts(counts);
    scatter_from_slice::<BUCKETS>(src, dst, counts, shift);
}

fn partition_from_initialized<const BUCKETS: usize>(
    src: &[i32],
    dst: &mut [MaybeUninit<i32>],
    shift: usize,
) {
    let mut counts = [0usize; BUCKETS];
    partition_from_slice::<BUCKETS>(src, dst, &mut counts, shift);
}

fn count_buckets<const BUCKETS: usize>(src: &[i32], counts: &mut [usize; BUCKETS], shift: usize) {
    counts.fill(0);
    let mask = (BUCKETS as u32) - 1;
    for &value in src {
        let key = ((value as u32) ^ 0x8000_0000) >> shift;
        counts[(key & mask) as usize] += 1;
    }
}

fn prefix_counts<const BUCKETS: usize>(counts: &mut [usize; BUCKETS]) {
    let mut sum = 0;
    for count in counts {
        let current = *count;
        *count = sum;
        sum += current;
    }
}

fn cumulative_counts<const BUCKETS: usize>(counts: &mut [usize; BUCKETS]) {
    let mut sum = 0;
    for count in counts {
        sum += *count;
        *count = sum;
    }
}

fn scatter_from_slice<const BUCKETS: usize>(
    src: &[i32],
    dst: &mut [MaybeUninit<i32>],
    counts: &mut [usize; BUCKETS],
    shift: usize,
) {
    let mask = (BUCKETS as u32) - 1;
    for &value in src {
        let key = ((value as u32) ^ 0x8000_0000) >> shift;
        let bucket = (key & mask) as usize;
        dst[counts[bucket]].write(value);
        counts[bucket] += 1;
    }
}

fn copy_into_uninit(src: &[i32], dst: &mut [MaybeUninit<i32>]) {
    for (src, dst) in src.iter().zip(dst) {
        dst.write(*src);
    }
}

fn output_lands_in_scratch<const BUCKETS: usize>(shift: usize) -> bool {
    let bits = radix_bits::<BUCKETS>();
    (shift.div_ceil(bits) + 1) % 2 == 0
}

const fn radix_bits<const BUCKETS: usize>() -> usize {
    BUCKETS.ilog2() as usize
}

fn uninit_vec(len: usize) -> Vec<MaybeUninit<i32>> {
    let mut values = Vec::with_capacity(len);
    unsafe {
        values.set_len(len);
    }
    values
}

fn initialized_as_uninit_mut(values: &mut [i32]) -> &mut [MaybeUninit<i32>] {
    unsafe { std::slice::from_raw_parts_mut(values.as_mut_ptr().cast(), values.len()) }
}

unsafe fn assume_init_slice(values: &[MaybeUninit<i32>]) -> &[i32] {
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast(), values.len()) }
}

unsafe fn assume_init_slice_mut(values: &mut [MaybeUninit<i32>]) -> &mut [i32] {
    unsafe { std::slice::from_raw_parts_mut(values.as_mut_ptr().cast(), values.len()) }
}

unsafe fn assume_init_vec(values: Vec<MaybeUninit<i32>>) -> Vec<i32> {
    let mut values = ManuallyDrop::new(values);
    unsafe { Vec::from_raw_parts(values.as_mut_ptr().cast(), values.len(), values.capacity()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Distribution;
    use rayon::prelude::*;

    #[test]
    fn test_uninit_msd_7bit() {
        assert_uninit_sort(par_msd_radix_sort_7bit_new_uninit);
    }

    #[test]
    fn test_uninit_msd_8bit() {
        assert_uninit_sort(par_msd_radix_sort_8bit_new_uninit);
    }

    fn assert_uninit_sort(sort: fn(&[i32]) -> Vec<i32>) {
        let values = Distribution::Random.values(100_000);
        let mut expected = values.clone();
        expected.par_sort_unstable();

        let sorted = sort(&values);
        assert_eq!(expected, sorted);
    }
}
