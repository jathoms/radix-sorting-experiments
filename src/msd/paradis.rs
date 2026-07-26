use rayon::prelude::*;

const BUCKETS: usize = 256;
const MASK: u32 = (BUCKETS as u32) - 1;
const COMPARISON_FALLBACK_THRESHOLD: usize = 8_000;
const NEAR_SORTED_INVERSION_DENOMINATOR: usize = 32;
const MIN_PARALLEL_ITEMS_PER_WORKER: usize = 32_768;
const MAX_REPAIR_ITERATIONS: usize = 64;

pub fn paradis_sort_i32_new(values: &[i32]) -> Vec<i32> {
    let mut output = vec![0; values.len()];
    paradis_sort_i32_to_output(values, &mut output);
    output
}

pub fn paradis_sort_i32_to_output(values: &[i32], output: &mut [i32]) {
    assert_eq!(values.len(), output.len());
    let workers = rayon::current_num_threads();
    paradis_sort_to_output_recursive(values, output, 24, workers, COMPARISON_FALLBACK_THRESHOLD);
}

#[doc(hidden)]
pub fn paradis_sort_i32_to_output_with_threshold(
    values: &[i32],
    output: &mut [i32],
    fallback_threshold: usize,
) {
    assert_eq!(values.len(), output.len());
    let workers = rayon::current_num_threads();
    paradis_sort_to_output_recursive(values, output, 24, workers, fallback_threshold);
}

pub fn paradis_sort_i32(values: &mut [i32]) {
    let workers = rayon::current_num_threads();
    paradis_sort_recursive(values, 24, workers, COMPARISON_FALLBACK_THRESHOLD);
}

fn paradis_sort_recursive(
    values: &mut [i32],
    shift: usize,
    workers: usize,
    fallback_threshold: usize,
) {
    if values.len() <= fallback_threshold {
        comparison_sort(values, workers);
        return;
    }

    let workers = effective_workers(values.len(), workers);
    if workers <= 1 {
        values.sort_unstable();
        return;
    }

    let scan = parallel_scan(values, shift, workers);
    if scan.sorted {
        return;
    }
    if scan.reversed {
        values.reverse();
        return;
    }
    if is_nearly_sorted(values.len(), scan.adjacent_inversions) {
        comparison_sort(values, workers);
        return;
    }

    let counts = scan.counts;
    if n_non_empty_buckets(&counts) <= 1 {
        if let Some(next_shift) = next_differing_byte_shift(scan.or_diff, shift) {
            paradis_sort_recursive(values, next_shift, workers, fallback_threshold);
        }
        return;
    }

    let (bucket_starts, bucket_ends) = paradis_partition(values, shift, workers, counts);

    if shift == 0 {
        return;
    }

    recurse_buckets(
        values,
        &bucket_starts,
        &bucket_ends,
        shift - 8,
        workers,
        fallback_threshold,
    );
}

fn paradis_sort_to_output_recursive(
    values: &[i32],
    output: &mut [i32],
    shift: usize,
    workers: usize,
    fallback_threshold: usize,
) {
    if values.len() <= fallback_threshold {
        output.copy_from_slice(values);
        comparison_sort(output, workers);
        return;
    }

    let workers = effective_workers(values.len(), workers);
    let scan = parallel_scan(values, shift, workers);
    if scan.sorted {
        output.copy_from_slice(values);
        return;
    }
    if scan.reversed {
        for (dst, src) in output.iter_mut().zip(values.iter().rev()) {
            *dst = *src;
        }
        return;
    }
    if is_nearly_sorted(values.len(), scan.adjacent_inversions) {
        output.copy_from_slice(values);
        comparison_sort(output, workers);
        return;
    }

    if n_non_empty_buckets(&scan.counts) <= 1 {
        if let Some(next_shift) = next_differing_byte_shift(scan.or_diff, shift) {
            paradis_sort_to_output_recursive(
                values,
                output,
                next_shift,
                workers,
                fallback_threshold,
            );
        } else {
            output.copy_from_slice(values);
        }
        return;
    }

    let (bucket_starts, bucket_ends) = bucket_bounds(scan.counts);
    partition_to_output(values, output, &bucket_starts, &scan, shift);

    if shift == 0 {
        return;
    }

    recurse_buckets(
        output,
        &bucket_starts,
        &bucket_ends,
        shift - 8,
        workers,
        fallback_threshold,
    );
}

fn paradis_partition(
    values: &mut [i32],
    shift: usize,
    workers: usize,
    counts: [usize; BUCKETS],
) -> ([usize; BUCKETS], [usize; BUCKETS]) {
    let (bucket_starts, bucket_ends) = bucket_bounds(counts);
    let mut bucket_heads = bucket_starts;
    let ptr = SendPtr(values.as_mut_ptr());
    let mut remaining = values.len();
    let mut stripe_heads = vec![[0usize; BUCKETS]; workers];
    let mut stripe_ends = vec![[0usize; BUCKETS]; workers];
    let mut repaired_heads = [0usize; BUCKETS];

    for _ in 0..MAX_REPAIR_ITERATIONS {
        if remaining == 0 {
            #[cfg(debug_assertions)]
            debug_assert_buckets(values, shift, &bucket_starts, &bucket_ends);
            return (bucket_starts, bucket_ends);
        }

        partition_for_permutation(
            &bucket_heads,
            &bucket_ends,
            &mut stripe_heads,
            &mut stripe_ends,
        );

        stripe_heads
            .par_iter_mut()
            .zip(stripe_ends.par_iter())
            .for_each(|(heads, ends)| unsafe {
                paradis_permute_worker(ptr, heads, ends, shift);
            });

        repaired_heads
            .par_iter_mut()
            .enumerate()
            .for_each(|(bucket, repaired_head)| unsafe {
                *repaired_head = paradis_repair_bucket(
                    ptr,
                    bucket,
                    &stripe_heads,
                    &stripe_ends,
                    bucket_ends[bucket],
                    shift,
                );
            });

        let mut new_remaining = 0;
        for bucket in 0..BUCKETS {
            bucket_heads[bucket] = repaired_heads[bucket];
            new_remaining += bucket_ends[bucket] - bucket_heads[bucket];
        }

        debug_assert!(new_remaining < remaining);
        if new_remaining >= remaining {
            comparison_sort(values, workers);
            return (bucket_starts, bucket_ends);
        }
        remaining = new_remaining;
    }

    comparison_sort(values, workers);
    (bucket_starts, bucket_ends)
}

unsafe fn paradis_permute_worker(
    ptr: SendPtr,
    heads: &mut [usize; BUCKETS],
    ends: &[usize; BUCKETS],
    shift: usize,
) {
    for bucket in 0..BUCKETS {
        let mut head = heads[bucket];

        while head < ends[bucket] {
            let mut value = unsafe { ptr.read(head) };
            let mut value_bucket = bucket_of(value, shift);

            while value_bucket != bucket && heads[value_bucket] < ends[value_bucket] {
                let target = heads[value_bucket];
                heads[value_bucket] += 1;
                unsafe { ptr.swap_with_value(target, &mut value) };
                value_bucket = bucket_of(value, shift);
            }

            if value_bucket == bucket {
                let first_wrong = heads[bucket];
                let displaced = unsafe { ptr.read(first_wrong) };
                unsafe {
                    ptr.write(head, displaced);
                    ptr.write(first_wrong, value);
                }
                heads[bucket] += 1;
            } else {
                unsafe { ptr.write(head, value) };
            }

            head += 1;
        }
    }
}

unsafe fn paradis_repair_bucket(
    ptr: SendPtr,
    bucket: usize,
    stripe_heads: &[[usize; BUCKETS]],
    stripe_ends: &[[usize; BUCKETS]],
    bucket_end: usize,
    shift: usize,
) -> usize {
    let mut tail = bucket_end;

    for worker in 0..stripe_heads.len() {
        let mut head = stripe_heads[worker][bucket];
        let end = stripe_ends[worker][bucket];

        while head < end && head < tail {
            let value = unsafe { ptr.read(head) };
            head += 1;

            if bucket_of(value, shift) != bucket {
                let wrong_index = head - 1;
                let mut found_replacement = false;

                while head < tail {
                    tail -= 1;
                    let candidate = unsafe { ptr.read(tail) };

                    if bucket_of(candidate, shift) == bucket {
                        unsafe {
                            ptr.write(head - 1, candidate);
                            ptr.write(tail, value);
                        }
                        found_replacement = true;
                        break;
                    }
                }

                if !found_replacement {
                    tail = wrong_index;
                }
            }
        }
    }

    tail
}

struct ScanStats {
    counts: [usize; BUCKETS],
    chunk_counts: Vec<[usize; BUCKETS]>,
    chunk_size: usize,
    or_diff: u32,
    adjacent_inversions: usize,
    sorted: bool,
    reversed: bool,
}

struct ChunkStats {
    counts: [usize; BUCKETS],
    first: i32,
    last: i32,
    first_key: u32,
    or_diff: u32,
    adjacent_inversions: usize,
    sorted: bool,
    reversed: bool,
}

fn parallel_scan(values: &[i32], shift: usize, workers: usize) -> ScanStats {
    let chunk_size = values.len().div_ceil(workers);
    let chunks = values
        .par_chunks(chunk_size)
        .map(|chunk| scan_chunk(chunk, shift))
        .collect::<Vec<_>>();

    let mut counts = [0usize; BUCKETS];
    let mut sorted = true;
    let mut reversed = true;
    let first_key = chunks[0].first_key;
    let mut or_diff = 0;
    let mut adjacent_inversions = 0;
    let mut previous_last = None;

    let mut chunk_counts = Vec::with_capacity(chunks.len());

    for chunk in chunks {
        let counts_for_chunk = chunk.counts;
        for (total, count) in counts.iter_mut().zip(counts_for_chunk) {
            *total += count;
        }
        chunk_counts.push(counts_for_chunk);

        if let Some(previous_last) = previous_last {
            sorted &= previous_last <= chunk.first;
            reversed &= previous_last >= chunk.first;
            adjacent_inversions += usize::from(previous_last > chunk.first);
        }
        sorted &= chunk.sorted;
        reversed &= chunk.reversed;
        or_diff |= chunk.or_diff | (chunk.first_key ^ first_key);
        adjacent_inversions += chunk.adjacent_inversions;
        previous_last = Some(chunk.last);
    }

    ScanStats {
        counts,
        chunk_counts,
        chunk_size,
        or_diff,
        adjacent_inversions,
        sorted,
        reversed,
    }
}

fn partition_to_output(
    values: &[i32],
    output: &mut [i32],
    bucket_starts: &[usize; BUCKETS],
    scan: &ScanStats,
    shift: usize,
) {
    let mut chunk_offsets = vec![[0usize; BUCKETS]; scan.chunk_counts.len()];
    let mut offsets = *bucket_starts;

    for (chunk_offsets, chunk_counts) in chunk_offsets.iter_mut().zip(&scan.chunk_counts) {
        for bucket in 0..BUCKETS {
            chunk_offsets[bucket] = offsets[bucket];
            offsets[bucket] += chunk_counts[bucket];
        }
    }

    let ptr = SendPtr(output.as_mut_ptr());
    values
        .par_chunks(scan.chunk_size)
        .zip(chunk_offsets.par_iter())
        .for_each(|(chunk, offsets)| unsafe {
            let mut offsets = *offsets;
            for &value in chunk {
                let bucket = bucket_of(value, shift);
                ptr.write(offsets[bucket], value);
                offsets[bucket] += 1;
            }
        });
}

fn scan_chunk(chunk: &[i32], shift: usize) -> ChunkStats {
    let mut counts = [0usize; BUCKETS];
    let mut sorted = true;
    let mut reversed = true;
    let mut previous = chunk[0];
    let first = previous;
    let first_key = radix_key(previous);
    let mut or_diff = 0;
    let mut adjacent_inversions = 0;
    counts[bucket_of_key(first_key, shift)] += 1;

    for &value in &chunk[1..] {
        adjacent_inversions += usize::from(previous > value);
        sorted &= previous <= value;
        reversed &= previous >= value;
        let key = radix_key(value);
        or_diff |= key ^ first_key;
        counts[bucket_of_key(key, shift)] += 1;
        previous = value;
    }

    ChunkStats {
        counts,
        first,
        last: previous,
        first_key,
        or_diff,
        adjacent_inversions,
        sorted,
        reversed,
    }
}

fn bucket_bounds(counts: [usize; BUCKETS]) -> ([usize; BUCKETS], [usize; BUCKETS]) {
    let mut starts = [0usize; BUCKETS];
    let mut ends = [0usize; BUCKETS];
    let mut sum = 0;

    for bucket in 0..BUCKETS {
        starts[bucket] = sum;
        sum += counts[bucket];
        ends[bucket] = sum;
    }

    (starts, ends)
}

fn partition_for_permutation(
    bucket_heads: &[usize; BUCKETS],
    bucket_ends: &[usize; BUCKETS],
    stripe_heads: &mut [[usize; BUCKETS]],
    stripe_ends: &mut [[usize; BUCKETS]],
) {
    let workers = stripe_heads.len();

    for bucket in 0..BUCKETS {
        let len = bucket_ends[bucket] - bucket_heads[bucket];
        let base = len / workers;
        let extra = len % workers;
        let mut cursor = bucket_heads[bucket];

        for worker in 0..workers {
            let stripe_len = base + usize::from(worker < extra);
            stripe_heads[worker][bucket] = cursor;
            cursor += stripe_len;
            stripe_ends[worker][bucket] = cursor;
        }
    }
}

fn recurse_buckets(
    values: &mut [i32],
    bucket_starts: &[usize; BUCKETS],
    bucket_ends: &[usize; BUCKETS],
    shift: usize,
    workers: usize,
    fallback_threshold: usize,
) {
    let worker_counts = partition_for_recursion(bucket_starts, bucket_ends, workers);
    let ptr = SendPtr(values.as_mut_ptr());
    let jobs = (0..BUCKETS)
        .filter_map(|bucket| {
            let start = bucket_starts[bucket];
            let len = bucket_ends[bucket] - start;
            (len > 1).then_some((start, len, worker_counts[bucket]))
        })
        .collect::<Vec<_>>();

    jobs.into_par_iter()
        .for_each(|(start, len, bucket_workers)| unsafe {
            let bucket_values = ptr.slice_mut(start, len);
            if bucket_workers <= 1 {
                bucket_values.sort_unstable();
            } else if len <= fallback_threshold {
                comparison_sort(bucket_values, bucket_workers);
            } else {
                paradis_sort_recursive(bucket_values, shift, bucket_workers, fallback_threshold);
            }
        });
}

fn partition_for_recursion(
    bucket_starts: &[usize; BUCKETS],
    bucket_ends: &[usize; BUCKETS],
    workers: usize,
) -> [usize; BUCKETS] {
    let mut work = [0.0f64; BUCKETS];
    let mut total_work = 0.0;

    for bucket in 0..BUCKETS {
        let len = (bucket_ends[bucket] - bucket_starts[bucket]) as f64;
        if len > 1.0 {
            let bucket_work = len * len.log(BUCKETS as f64);
            work[bucket] = bucket_work;
            total_work += bucket_work;
        }
    }

    let mut worker_counts = [1usize; BUCKETS];
    if total_work == 0.0 || workers <= 1 {
        return worker_counts;
    }

    let mut remaining_workers = workers;
    let mut remaining_work = total_work;

    for bucket in 0..BUCKETS {
        if work[bucket] == 0.0 {
            continue;
        }

        let allocated = ((work[bucket] / remaining_work) * remaining_workers as f64)
            .round()
            .max(1.0) as usize;
        let allocated = allocated.min(remaining_workers).max(1);
        worker_counts[bucket] = allocated;
        remaining_workers = remaining_workers.saturating_sub(allocated);
        remaining_work -= work[bucket];

        if remaining_workers == 0 {
            break;
        }
    }

    worker_counts
}

fn effective_workers(len: usize, requested_workers: usize) -> usize {
    requested_workers
        .min(len.div_ceil(MIN_PARALLEL_ITEMS_PER_WORKER).max(1))
        .max(1)
}

fn comparison_sort(values: &mut [i32], workers: usize) {
    if workers > 1 && values.len() >= MIN_PARALLEL_ITEMS_PER_WORKER {
        values.par_sort_unstable();
    } else {
        values.sort_unstable();
    }
}

fn bucket_of(value: i32, shift: usize) -> usize {
    bucket_of_key(radix_key(value), shift)
}

fn bucket_of_key(key: u32, shift: usize) -> usize {
    ((key >> shift) & MASK) as usize
}

fn radix_key(value: i32) -> u32 {
    (value as u32) ^ 0x8000_0000
}

fn next_differing_byte_shift(or_diff: u32, shift: usize) -> Option<usize> {
    let mut next = shift;
    while next >= 8 {
        next -= 8;
        if ((or_diff >> next) & MASK) != 0 {
            return Some(next);
        }
    }
    None
}

fn n_non_empty_buckets(counts: &[usize; BUCKETS]) -> usize {
    counts.iter().filter(|&&count| count != 0).count()
}

fn is_nearly_sorted(len: usize, adjacent_inversions: usize) -> bool {
    adjacent_inversions <= len / NEAR_SORTED_INVERSION_DENOMINATOR
}

#[cfg(debug_assertions)]
fn debug_assert_buckets(
    values: &[i32],
    shift: usize,
    bucket_starts: &[usize; BUCKETS],
    bucket_ends: &[usize; BUCKETS],
) {
    for bucket in 0..BUCKETS {
        for index in bucket_starts[bucket]..bucket_ends[bucket] {
            assert_eq!(
                bucket_of(values[index], shift),
                bucket,
                "value {} at index {index} is in bucket {}, expected {bucket} at shift {shift}",
                values[index],
                bucket_of(values[index], shift)
            );
        }
    }
}

#[derive(Clone, Copy)]
struct SendPtr(*mut i32);

unsafe impl Send for SendPtr {}
unsafe impl Sync for SendPtr {}

impl SendPtr {
    unsafe fn read(self, index: usize) -> i32 {
        unsafe { *self.0.add(index) }
    }

    unsafe fn write(self, index: usize, value: i32) {
        unsafe { *self.0.add(index) = value };
    }

    unsafe fn swap_with_value(self, index: usize, value: &mut i32) {
        unsafe { std::mem::swap(value, &mut *self.0.add(index)) };
    }

    unsafe fn slice_mut<'a>(self, start: usize, len: usize) -> &'a mut [i32] {
        unsafe { std::slice::from_raw_parts_mut(self.0.add(start), len) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Distribution;

    #[test]
    fn test_paradis_sort_i32() {
        assert_paradis_sorts("random", Distribution::Random.values(500_000));
    }

    #[test]
    fn test_paradis_sort_i32_to_output() {
        let values = Distribution::Random.values(500_000);
        let mut expected = values.clone();
        let mut actual = vec![0; values.len()];

        expected.sort_unstable();
        paradis_sort_i32_to_output(&values, &mut actual);

        assert_eq!(expected, actual);
    }

    #[test]
    fn test_paradis_patterns() {
        assert_paradis_sorts("sorted", (0..100_000).collect());
        assert_paradis_sorts("reversed", (0..100_000).rev().collect());
        assert_paradis_sorts("few_unique", (0..100_000).map(|i| (i % 128) - 64).collect());
        assert_paradis_sorts(
            "same_high_byte",
            (0..100_000)
                .map(|i| pseudo_random_i32(i as u32) & 0x00ff_ffff)
                .collect(),
        );
    }

    #[test]
    fn test_two_stripe_reversed_repair_boundary() {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        let values = (0..100_000).rev().collect::<Vec<_>>();
        let mut expected = values.clone();
        let mut actual = values;

        expected.sort_unstable();
        pool.install(|| paradis_sort_recursive(&mut actual, 24, 2, COMPARISON_FALLBACK_THRESHOLD));

        if expected != actual {
            let index = expected
                .iter()
                .zip(&actual)
                .position(|(expected, actual)| expected != actual)
                .unwrap();
            panic!(
                "first mismatch at {index}: expected {}, got {}",
                expected[index], actual[index]
            );
        }
    }

    fn assert_paradis_sorts(name: &str, values: Vec<i32>) {
        let mut expected = values.clone();
        let mut actual = values;

        expected.sort_unstable();
        paradis_sort_i32(&mut actual);

        if expected != actual {
            let index = expected
                .iter()
                .zip(&actual)
                .position(|(expected, actual)| expected != actual)
                .unwrap();
            panic!(
                "{name}: first mismatch at {index}: expected {}, got {}",
                expected[index], actual[index]
            );
        }
    }

    fn pseudo_random_i32(i: u32) -> i32 {
        i.wrapping_mul(1_664_525).wrapping_add(1_013_904_223) as i32
    }
}
