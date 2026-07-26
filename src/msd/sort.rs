pub fn single_msd_pass_into_quicksort_new(values: &mut [i32]) -> Vec<i32> {
    let mut dst = vec![0; values.len()];
    single_msd_pass_into_quicksort(values, &mut dst);
    dst
}
pub fn single_msd_pass_into_quicksort(values: &mut [i32], dst: &mut [i32]) {
    let mut counts = [0usize; 1024];
    msd_radix_partition::<1024>(values, dst, &mut counts, i32_first_shift::<1024>());
    par_sort_unstable(dst);
}

pub fn msd_radix_sort_8bit(values: &mut [i32]) {
    let len = values.len();
    let mut dst = vec![0i32; len];
    msd_radix_sort_8bit_in(values, &mut dst, 24);
}
pub fn msd_radix_sort_8bit_in(src: &mut [i32], dst: &mut [i32], shift: usize) {
    // println!("shift: {shift}, src: {:?}", src);
    if src.len() <= 1 {
        return;
    }
    let mut counts = [0usize; 256];

    for value in src.iter() {
        let key = ((*value as u32) ^ 0x8000_0000) >> shift;
        let bucket = (key & 0xff) as usize;
        // println!("bucket:{bucket}");
        counts[bucket] += 1;
    }
    // println!("initial counts: {:?}", counts);

    let mut sum = 0;
    for count in &mut counts {
        let current = *count;
        *count = sum;
        sum += current;
    }

    for value in &mut *src {
        let key = ((*value as u32) ^ 0x8000_0000) >> shift;
        let bucket = (key & 0xff) as usize;
        dst[counts[bucket]] = *value;
        counts[bucket] += 1;
    }

    // println!("copying {dst:?} into {src:?}");
    src.copy_from_slice(dst);

    if shift == 0 {
        return;
    };

    let mut start = 0;
    for end in counts {
        if end - start < 2 {
            start = end;
            continue;
        }
        let s = &mut src[start..end];
        let d = &mut dst[start..end];
        // println!("s (dst[{}..{}]): {:?}", start, end, s);
        msd_radix_sort_8bit_in(s, d, shift - 8);
        start = end;
    }
}

pub const BUCKETS_8: usize = 1 << 8;
pub const BUCKETS_7: usize = 1 << 7;
pub const BUCKETS_6: usize = 1 << 6;
pub const BUCKETS_5: usize = 1 << 5;
pub const BUCKETS_4: usize = 1 << 4;
pub const BUCKETS_3: usize = 1 << 3;
pub const BUCKETS_2: usize = 1 << 2;
pub const BUCKETS_1: usize = 1 << 1;

const DEFAULT_FALLBACK_THRESHOLD: usize = 50_000;
const PARALLEL_PARTITION_THRESHOLD: usize = 250_000;

pub fn par_msd_radix_sort_8bit(values: &mut [i32]) {
    let mut dst = vec![0i32; values.len()];
    par_msd_radix_sort_in::<BUCKETS_8, BUCKETS_8>(values, &mut dst, 24);
}

pub fn par_msd_radix_sort_8bit_new(values: &mut [i32]) -> Vec<i32> {
    let mut dst = vec![0i32; values.len()];
    par_msd_radix_sort_in::<BUCKETS_8, BUCKETS_8>(values, &mut dst, 24);
    dst.copy_from_slice(values);
    dst
}
pub fn par_msd_radix_sort_7bit_new(values: &mut [i32]) -> Vec<i32> {
    let mut dst = vec![0i32; values.len()];
    par_msd_radix_sort_in::<BUCKETS_7, BUCKETS_4>(values, &mut dst, 25);
    dst.copy_from_slice(values);
    dst
}

pub fn par_msd_radix_sort_7bit(values: &mut [i32]) {
    let mut dst = vec![0i32; values.len()];
    par_msd_radix_sort_in::<BUCKETS_7, BUCKETS_4>(values, &mut dst, 24);
}

pub fn par_sort_unstable_new(values: &mut [i32]) -> Vec<i32> {
    values.par_sort_unstable();
    values.to_vec()
}

pub fn par_sort_unstable(values: &mut [i32]) {
    values.par_sort_unstable();
}

const fn radix_bits<const BUCKETS: usize>() -> usize {
    BUCKETS.ilog2() as usize
}

pub const fn i32_first_shift<const BUCKETS: usize>() -> usize {
    32 - radix_bits::<BUCKETS>()
}

pub fn par_msd_radix_sort_in<const BUCKETS: usize, const RESIDUAL_BUCKETS: usize>(
    src: &mut [i32],
    dst: &mut [i32],
    shift: usize,
) {
    par_msd_radix_sort_in_threshold_impl::<BUCKETS, RESIDUAL_BUCKETS>(
        src,
        dst,
        shift,
        DEFAULT_FALLBACK_THRESHOLD,
    );
}

pub fn par_msd_radix_sort_in_threshold<const BUCKETS: usize, const RESIDUAL_BUCKETS: usize>(
    src: &mut [i32],
    dst: &mut [i32],
    shift: usize,
    fallback_threshold: usize,
) {
    par_msd_radix_sort_in_threshold_impl::<BUCKETS, RESIDUAL_BUCKETS>(
        src,
        dst,
        shift,
        fallback_threshold,
    );
}

fn par_msd_radix_sort_in_threshold_impl<const BUCKETS: usize, const RESIDUAL_BUCKETS: usize>(
    src: &mut [i32],
    dst: &mut [i32],
    shift: usize,
    fallback_threshold: usize,
) {
    // println!("shift: {shift}, src: {:?}", src);

    let bits = radix_bits::<BUCKETS>();

    if src.len() <= 1 {
        return;
    }

    let mut counts = [0usize; BUCKETS];
    if src.len() >= PARALLEL_PARTITION_THRESHOLD {
        par_msd_radix_partition::<BUCKETS>(src, dst, &mut counts, shift);
    } else {
        msd_radix_partition::<BUCKETS>(src, dst, &mut counts, shift);
    }

    // println!("copying {dst:?} into {src:?}");

    if shift == 0 {
        return;
    };

    rayon::scope(|scope| {
        let mut src_rest = src;
        let mut dst_rest = dst;
        let mut start = 0;

        for end in counts {
            let bucket_len = end - start;
            let (bucket_src, next_src) = src_rest.split_at_mut(bucket_len);
            let (bucket_dst, next_dst) = dst_rest.split_at_mut(bucket_len);

            if bucket_len == 1 && output_lands_in_src::<BUCKETS>(shift) {
                bucket_src.copy_from_slice(bucket_dst);
            } else if bucket_len >= 2 {
                if bucket_len < fallback_threshold {
                    scope.spawn(|_| {
                        bucket_dst.sort_unstable();
                        if output_lands_in_src::<BUCKETS>(shift) {
                            bucket_src.copy_from_slice(bucket_dst);
                        }
                    })
                } else {
                    scope.spawn(|_| {
                        if shift < bits {
                            par_msd_radix_sort_final::<RESIDUAL_BUCKETS>(bucket_dst, bucket_src);
                        } else {
                            par_msd_radix_sort_in_threshold_impl::<BUCKETS, RESIDUAL_BUCKETS>(
                                bucket_dst,
                                bucket_src,
                                shift - bits,
                                fallback_threshold,
                            );
                        }
                    });
                }
            }

            src_rest = next_src;
            dst_rest = next_dst;
            start = end;
        }
    });
}

pub fn msd_radix_partition<const BUCKETS: usize>(
    src: &[i32],
    dst: &mut [i32],
    counts: &mut [usize; BUCKETS],
    shift: usize,
) {
    let mask = (BUCKETS as u32) - 1;

    for value in src.iter() {
        let key = ((*value as u32) ^ 0x8000_0000) >> shift;
        let bucket = (key & mask) as usize;
        counts[bucket] += 1;
    }

    let mut sum = 0;
    for count in counts.iter_mut() {
        let current = *count;
        *count = sum;
        sum += current;
    }

    for value in src {
        let key = ((*value as u32) ^ 0x8000_0000) >> shift;
        let bucket = (key & mask) as usize;
        dst[counts[bucket]] = *value;
        counts[bucket] += 1;
    }
}

pub fn par_msd_radix_partition<const BUCKETS: usize>(
    src: &mut [i32],
    dst: &mut [i32],
    counts: &mut [usize; BUCKETS],
    shift: usize,
) {
    let n_chunks = rayon::current_num_threads();
    let chunk_size = src.len().div_ceil(n_chunks);
    let mask = (BUCKETS as u32) - 1;

    let mut chunked_counts = vec![[0usize; BUCKETS]; n_chunks];
    let mut chunked_offsets = vec![[0usize; BUCKETS]; n_chunks];

    chunked_counts
        .par_iter_mut()
        .zip(src.par_chunks(chunk_size))
        .for_each(|(counts, chunk)| {
            for &value in chunk {
                let key = ((value as u32) ^ 0x8000_0000) >> shift;
                counts[(key & mask) as usize] += 1;
            }
        });

    for bucket in 0..BUCKETS {
        let count = chunked_counts.iter().map(|chunk| chunk[bucket]).sum();
        counts[bucket] = count;
    }

    let mut sum = 0;
    for count in counts.iter_mut() {
        let current = *count;
        *count = sum;
        sum += current;
    }

    let mut next_offsets = *counts;
    for (chunk_counts, chunk_offsets) in chunked_counts.iter().zip(&mut chunked_offsets) {
        *chunk_offsets = next_offsets;
        for bucket in 0..BUCKETS {
            next_offsets[bucket] += chunk_counts[bucket];
        }
    }

    *counts = next_offsets;

    let dst_ptr = dst.as_mut_ptr() as usize;
    src.par_chunks(chunk_size)
        .zip(chunked_offsets.par_iter_mut())
        .for_each(|(chunk, offsets)| {
            for &value in chunk {
                let key = ((value as u32) ^ 0x8000_0000) >> shift;
                let bucket = (key & mask) as usize;
                let offset = offsets[bucket];
                unsafe {
                    (dst_ptr as *mut i32).add(offset).write(value);
                }
                offsets[bucket] += 1;
            }
        });
}

fn par_msd_radix_sort_final<const BUCKETS: usize>(src: &mut [i32], dst: &mut [i32]) {
    if src.len() <= 1 {
        return;
    }

    let mut counts = [0usize; BUCKETS];
    let mask = (BUCKETS as u32) - 1;

    for value in src.iter() {
        let key = (*value as u32) ^ 0x8000_0000;
        counts[(key & mask) as usize] += 1;
    }

    let mut sum = 0;
    for count in &mut counts {
        let current = *count;
        *count = sum;
        sum += current;
    }

    for value in src {
        let key = (*value as u32) ^ 0x8000_0000;
        let bucket = (key & mask) as usize;
        dst[counts[bucket]] = *value;
        counts[bucket] += 1;
    }
}

fn output_lands_in_src<const BUCKETS: usize>(shift: usize) -> bool {
    let bits = radix_bits::<BUCKETS>();
    (shift.div_ceil(bits) + 1) % 2 == 0
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::Distribution;

    #[test]
    fn test_msd() {
        let v = Distribution::Random.values(1234);
        // dbg!(&v);
        let mut v1 = v.clone();
        let mut v2 = v.clone();

        v1.sort_unstable();
        msd_radix_sort_8bit(&mut v2);
        // panic!();

        assert_eq!(v1, v2)
    }
    #[test]
    fn test_par_msd() {
        assert_par_msd::<BUCKETS_1, BUCKETS_1>(100_000);
        assert_par_msd::<BUCKETS_2, BUCKETS_2>(100_000);
        assert_par_msd::<BUCKETS_3, BUCKETS_2>(100_000);
        assert_par_msd::<BUCKETS_4, BUCKETS_4>(100_000);
        assert_par_msd::<BUCKETS_5, BUCKETS_2>(100_000);
        assert_par_msd::<BUCKETS_6, BUCKETS_2>(100_000);
        assert_par_msd::<BUCKETS_7, BUCKETS_4>(100_000);
        assert_par_msd::<BUCKETS_8, BUCKETS_8>(100_000);
    }

    fn assert_par_msd<const BUCKETS: usize, const RESIDUAL_BUCKETS: usize>(size: usize) {
        let v = Distribution::Random.values(size);
        let mut v1 = v.clone();
        let mut v2 = v.clone();
        let mut dst = v.clone();

        v1.par_sort_unstable();
        par_msd_radix_sort_in::<BUCKETS, RESIDUAL_BUCKETS>(
            &mut v2,
            &mut dst,
            i32_first_shift::<BUCKETS>(),
        );

        let sorted = if output_lands_in_src::<BUCKETS>(i32_first_shift::<BUCKETS>()) {
            &v2
        } else {
            &dst
        };

        assert_eq!(&v1, sorted, "failed for {BUCKETS} buckets");
    }
}
use rayon::prelude::*;
