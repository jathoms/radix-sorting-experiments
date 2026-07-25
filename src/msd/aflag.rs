use core::slice;

use crate::msd::sort::{BUCKETS_4, BUCKETS_7, i32_first_shift, msd_radix_partition};

pub fn msd_inplace(src: &mut [i32]) {
    for shift in [0, 8, 16, 24] {
        msd_inplace_one_pass(src, shift);
    }
}
pub fn msd_once_then_inplace_new(values: &mut [i32]) -> Vec<i32> {
    let mut dst = vec![0; values.len()];
    msd_once_then_inplace(values, &mut dst);
    dst
}

pub fn msd_once_then_inplace(src: &mut [i32], dst: &mut [i32]) {
    let mut counts = [0; 256];
    msd_radix_partition::<256>(src, dst, &mut counts, 24);

    rayon::scope(|scope| {
        let mut dst_rest = dst;
        let mut start = 0;

        for end in counts {
            let bucket_len = end - start;
            let (bucket_dst, next_dst) = dst_rest.split_at_mut(bucket_len);

            if bucket_len >= 2 {
                if bucket_len < 75_000 {
                    scope.spawn(|_| {
                        bucket_dst.sort_unstable();
                    })
                } else {
                    scope.spawn(|_| {
                        par_msd_inplace(bucket_dst, 16);
                    });
                }
            }

            dst_rest = next_dst;
            start = end;
        }
    });
}

pub fn msd_once_then_inplace_eager(src: &mut [i32], dst: &mut [i32]) {
    let mut counts = [0; BUCKETS_7];
    msd_radix_partition::<BUCKETS_7>(src, dst, &mut counts, i32_first_shift::<BUCKETS_7>());

    rayon::scope(|scope| {
        let mut dst_rest = dst;
        let mut start = 0;

        for end in counts {
            let bucket_len = end - start;
            let (bucket_dst, next_dst) = dst_rest.split_at_mut(bucket_len);

            if bucket_len >= 2 {
                if bucket_len < 75_000 {
                    scope.spawn(|_| {
                        bucket_dst.sort_unstable();
                    })
                } else {
                    scope.spawn(|_| {
                        par_msd_inplace_eager::<BUCKETS_7, BUCKETS_4>(bucket_dst, 18);
                    });
                }
            }

            dst_rest = next_dst;
            start = end;
        }
    });
}

pub fn par_msd_inplace(src: &mut [i32], shift: usize) {
    let end = msd_inplace_one_pass(src, shift);
    if shift == 0 {
        return;
    }

    rayon::scope(|scope| {
        let mut src_rest = src;
        let mut start = 0;

        for b_end in end {
            let bucket_len = b_end - start;
            let (bucket_src, next_src) = src_rest.split_at_mut(bucket_len);

            if bucket_len > 1 {
                // println!(
                //     "({shift} from size {}: ({}) spawning for size {}",
                //     bucket_len,
                //     shift - 8,
                //     bucket_src.len()
                // );
                scope.spawn(|_| {
                    par_msd_inplace(bucket_src, shift - 8);
                });
            }

            src_rest = next_src;
            start = b_end;
        }
    });
}

pub fn msd_inplace_one_pass(src: &mut [i32], shift: usize) -> [usize; 256] {
    const BUCKETS: usize = 256;
    const MASK: u32 = (BUCKETS as u32) - 1;
    let mut counts = [0; BUCKETS];
    for value in src.iter() {
        let key = ((*value as u32) ^ 0x8000_0000) >> shift;
        let bucket = (key & MASK) as usize;
        counts[bucket] += 1;
    }

    let mut head = counts;
    let mut end = [0; BUCKETS];

    let mut sum = 0;
    for (count, v_end) in head.iter_mut().zip(end.iter_mut()) {
        let current = *count;
        *count = sum;
        sum += current;
        *v_end = sum;
    }
    for b in 0..BUCKETS {
        while head[b] < end[b] {
            let mut x = src[head[b]];
            loop {
                let key = ((x as u32) ^ 0x8000_0000) >> shift;
                let x_bucket = (key & MASK) as usize;
                if x_bucket == b {
                    break;
                };
                let x_head = &mut head[x_bucket];
                std::mem::swap(&mut x, &mut src[*x_head]);
                *x_head += 1;
            }
            src[head[b]] = x;
            head[b] += 1;
        }
    }
    end
}

pub fn par_msd_inplace_eager<const BUCKETS: usize, const RESIDUAL_BUCKETS: usize>(
    src: &mut [i32],
    shift: usize,
) {
    let bits = radix_bits::<BUCKETS>();
    let mask = (BUCKETS as u32) - 1;
    let mut counts = [0; BUCKETS];
    for value in src.iter() {
        let key = ((*value as u32) ^ 0x8000_0000) >> shift;
        let bucket = (key & mask) as usize;
        counts[bucket] += 1;
    }

    let mut head = counts;
    let mut end = [0; BUCKETS];

    let mut sum = 0;
    for (count, v_end) in head.iter_mut().zip(end.iter_mut()) {
        let current = *count;
        *count = sum;
        sum += current;
        *v_end = sum;
    }
    let start = head.clone();

    rayon::scope(|scope| {
        for b in 0..BUCKETS {
            let b_end = end[b];
            let b_start = start[b];
            if head[b] == b_end {
                continue;
            }
            while head[b] < b_end {
                let mut x = src[head[b]];
                loop {
                    let key = ((x as u32) ^ 0x8000_0000) >> shift;
                    let x_bucket = (key & mask) as usize;
                    if x_bucket == b {
                        break;
                    };
                    let x_head = &mut head[x_bucket];
                    std::mem::swap(&mut x, &mut src[*x_head]);
                    *x_head += 1;
                    let x_end = end[x_bucket];
                    if *x_head == x_end && shift > 0 {
                        let x_start = start[x_bucket];
                        let x_bucket_len = x_end - x_start;
                        if x_bucket_len <= 1 {
                            continue;
                        }
                        unsafe {
                            let x_bucket_ptr = src.as_mut_ptr().add(x_start);
                            let x_bucket_slice =
                                slice::from_raw_parts_mut(x_bucket_ptr, x_bucket_len);
                            if x_bucket_len < 75_000 {
                                scope.spawn(|_| {
                                    x_bucket_slice.sort_unstable();
                                })
                            } else if shift < bits {
                                scope.spawn(|_| {
                                    msd_inplace_one_pass_generic::<RESIDUAL_BUCKETS>(
                                        x_bucket_slice,
                                        0,
                                    );
                                });
                            } else {
                                scope.spawn(|_| {
                                    par_msd_inplace_eager::<BUCKETS, RESIDUAL_BUCKETS>(
                                        x_bucket_slice,
                                        shift - bits,
                                    );
                                });
                            }
                        };
                    }
                }
                src[head[b]] = x;
                head[b] += 1;
            }
            let b_bucket_len = b_end - b_start;
            if b_bucket_len <= 1 {
                continue;
            }
            if shift == 0 {
                continue;
            }
            unsafe {
                let b_bucket_ptr = src.as_mut_ptr().add(b_start);
                let b_bucket_slice = slice::from_raw_parts_mut(b_bucket_ptr, b_bucket_len);
                if b_bucket_len < 75_000 {
                    scope.spawn(|_| {
                        b_bucket_slice.sort_unstable();
                    })
                } else if shift < bits {
                    scope.spawn(|_| {
                        msd_inplace_one_pass_generic::<RESIDUAL_BUCKETS>(b_bucket_slice, 0);
                    });
                } else {
                    scope.spawn(|_| {
                        par_msd_inplace_eager::<BUCKETS, RESIDUAL_BUCKETS>(
                            b_bucket_slice,
                            shift - bits,
                        );
                    });
                }
            };
        }
    });
}

fn msd_inplace_one_pass_generic<const BUCKETS: usize>(
    src: &mut [i32],
    shift: usize,
) -> [usize; BUCKETS] {
    let mask = (BUCKETS as u32) - 1;
    let mut counts = [0; BUCKETS];
    for value in src.iter() {
        let key = ((*value as u32) ^ 0x8000_0000) >> shift;
        let bucket = (key & mask) as usize;
        counts[bucket] += 1;
    }

    let mut head = counts;
    let mut end = [0; BUCKETS];

    let mut sum = 0;
    for (count, v_end) in head.iter_mut().zip(end.iter_mut()) {
        let current = *count;
        *count = sum;
        sum += current;
        *v_end = sum;
    }
    for b in 0..BUCKETS {
        while head[b] < end[b] {
            let mut x = src[head[b]];
            loop {
                let key = ((x as u32) ^ 0x8000_0000) >> shift;
                let x_bucket = (key & mask) as usize;
                if x_bucket == b {
                    break;
                };
                let x_head = &mut head[x_bucket];
                std::mem::swap(&mut x, &mut src[*x_head]);
                *x_head += 1;
            }
            src[head[b]] = x;
            head[b] += 1;
        }
    }
    end
}

const fn radix_bits<const BUCKETS: usize>() -> usize {
    BUCKETS.ilog2() as usize
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::Distribution;

    #[test]
    fn test_single_pass() {
        const BUCKETS: usize = 256;
        const MASK: i32 = (BUCKETS as i32) - 1;
        let v = Distribution::Random.values(10000);
        let v = v.into_iter().map(|val: i32| val & MASK).collect::<Vec<_>>();
        // dbg!(&v);
        let mut v1 = v.clone();
        let mut v2 = v.clone();

        v1.sort_unstable();
        msd_inplace_one_pass(v2.as_mut_slice(), 0);

        assert_eq!(v1, v2)
    }
    #[test]
    fn test_par_inplace() {
        let v = Distribution::Random.values(5000);
        // dbg!(&v);
        let mut v1 = v.clone();
        let mut v2 = v.clone();

        v1.sort_unstable();
        par_msd_inplace(&mut v2, 24);

        assert_eq!(v1, v2)
    }
    #[test]
    fn test_hybrid_initial() {
        let v = Distribution::Random.values(5000);
        let mut v1 = v.clone();
        let mut v2 = v.clone();

        v1.sort_unstable();
        v2 = msd_once_then_inplace_new(&mut v2);

        assert_eq!(v1, v2)
    }
    #[test]
    fn test_eager() {
        let v = Distribution::Random.values(500000);
        let mut v1 = v.clone();
        let mut v2 = v.clone();

        v1.sort_unstable();
        par_msd_inplace_eager::<BUCKETS_7, BUCKETS_4>(&mut v2, i32_first_shift::<BUCKETS_7>());

        assert_eq!(v1, v2)
    }
}
