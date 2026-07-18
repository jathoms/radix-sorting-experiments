use rand::SeedableRng;
use rand::{rngs::StdRng, seq::SliceRandom};
use rayon::prelude::*;
use rdxsort::RdxSort;

const RADIX_11_BUCKETS: usize = 1 << 11;
const RADIX_10_BUCKETS: usize = 1 << 10;

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub enum Distribution {
    Random,
    Sorted,
    Reversed,
    FewUnique,
}

impl Distribution {
    pub const ALL: &[Self] = &[Self::Random];
    // Self::Sorted, Self::Reversed, Self::FewUnique];

    pub fn name(self) -> &'static str {
        match self {
            Self::Random => "random",
            Self::Sorted => "sorted",
            Self::Reversed => "reversed",
            Self::FewUnique => "few_unique",
        }
    }

    pub fn values(self, size: usize) -> Vec<i32> {
        let mut values = (0..size)
            .map(|i| (i as i32).wrapping_mul(32).wrapping_sub(5))
            .collect::<Vec<_>>();

        match self {
            Self::Random => {
                let mut rng = StdRng::seed_from_u64(41);
                values.shuffle(&mut rng);
                values
            }
            Self::Sorted => values,
            Self::Reversed => {
                values.reverse();
                values
            }
            Self::FewUnique => {
                let mut rng = StdRng::seed_from_u64(41);
                values.iter_mut().for_each(|value| *value %= 128);
                values.shuffle(&mut rng);
                values
            }
        }
    }
}

pub fn sort_unstable(values: &mut [i32]) {
    values.sort_unstable();
}

pub fn par_sort_unstable(values: &mut [i32]) {
    values.par_sort_unstable();
}

pub fn rdxsort(values: &mut [i32]) {
    values.rdxsort();
}

pub fn radix_sort_i32(values: &mut [i32]) {
    if values.len() <= 1 {
        return;
    }

    let len = values.len();
    let mut src = values.to_vec();
    let mut dst = vec![0i32; len];

    for shift in [0, 8, 16, 24] {
        let mut counts = [0usize; 256];

        for &value in &src {
            let key = ((value as u32) ^ 0x8000_0000) >> shift;
            counts[(key & 0xff) as usize] += 1;
        }

        let mut sum = 0;
        for count in &mut counts {
            let current = *count;
            *count = sum;
            sum += current;
        }

        for &value in &src {
            let key = ((value as u32) ^ 0x8000_0000) >> shift;
            let bucket = (key & 0xff) as usize;
            dst[counts[bucket]] = value;
            counts[bucket] += 1;
        }

        std::mem::swap(&mut src, &mut dst);
    }

    values.copy_from_slice(&src);
}
pub fn par_radix_sort_i32_8bit(values: &mut [i32]) {
    if values.len() <= 1 {
        return;
    }
    let chunk_size = values.len().div_ceil(rayon::current_num_threads());

    let len = values.len();
    let mut src = values.to_vec();
    let mut dst = vec![0i32; len];

    for shift in [0, 8, 16, 24] {
        // each chunked_counts[i] is a histogram over all buckets.
        let chunked_counts: Vec<[usize; 256]> = src
            .par_chunks(chunk_size)
            .map(|chunk| {
                let mut counts = [0usize; 256];

                for &value in chunk {
                    let key = ((value as u32) ^ 0x8000_0000) >> shift;
                    counts[(key & 0xff) as usize] += 1;
                }
                counts
            })
            .collect();
        // println!("chunked_counts: {:?}", &chunked_counts);
        // dbg!(&chunked_counts.len());
        // dbg!(&chunked_counts);

        let mut total_counts = [0; 256];
        for chunk in &chunked_counts {
            for (i, n) in chunk.iter().enumerate() {
                total_counts[i] += n;
            }
        }

        let mut sum = 0;
        for count in &mut total_counts {
            let current = *count;
            *count = sum;
            sum += current;
        }
        // println!("total counts: {:?}", &total_counts);

        // we need to know for each chunk: where do we start writing for each bucket, and increment
        // that cell on each write

        let mut chunked_offsets = vec![[0; 256]; chunked_counts.len()];
        for bucket in 0..256 {
            let mut bucket_offset = 0;
            for (chunk_idx, chunk) in chunked_counts.iter().enumerate() {
                chunked_offsets[chunk_idx][bucket] = bucket_offset + total_counts[bucket];
                bucket_offset += chunk[bucket]
            }
        }

        // println!("chunked_offsets: {:?}", &chunked_offsets);

        let dst_ptr = dst.as_mut_ptr() as usize;
        src.par_chunks(chunk_size)
            .enumerate()
            .for_each(|(chunk_idx, chunk)| {
                let mut offsets = chunked_offsets[chunk_idx];
                for &value in chunk {
                    let key = ((value as u32) ^ 0x8000_0000) >> shift;
                    let bucket = (key & 0xff) as usize;
                    let offset = offsets[bucket];
                    unsafe {
                        (dst_ptr as *mut i32).add(offset).write(value);
                    }
                    offsets[bucket] += 1;
                }
            });

        std::mem::swap(&mut src, &mut dst);
        // println!("new src after swap at shift {}: {:?}", shift, src)
    }

    values.copy_from_slice(&src);
}

pub fn par_radix_sort_single_pass<const BUCKETS: usize>(
    src: &[i32],
    dst: &mut [i32],
    chunk_size: usize,
    shift: usize,
) {
    if src.len() <= 1 {
        return;
    }
    let mask = (BUCKETS as u32) - 1;

    let chunked_counts: Vec<[usize; BUCKETS]> = src
        .par_chunks(chunk_size)
        .map(|chunk| {
            let mut counts = [0usize; BUCKETS];

            for &value in chunk {
                let key = ((value as u32) ^ 0x8000_0000) >> shift;
                let bucket = (key & mask) as usize;
                counts[bucket] += 1;
            }
            counts
        })
        .collect();

    let mut total_counts = [0; BUCKETS];
    for chunk in &chunked_counts {
        for (i, n) in chunk.iter().enumerate() {
            total_counts[i] += n;
        }
    }

    let mut sum = 0;
    for count in &mut total_counts {
        let current = *count;
        *count = sum;
        sum += current;
    }

    let mut chunked_offsets = vec![[0; BUCKETS]; chunked_counts.len()];
    let mut next_offsets = total_counts;

    for (chunk_idx, chunk) in chunked_counts.iter().enumerate() {
        chunked_offsets[chunk_idx] = next_offsets;

        for bucket in 0..BUCKETS {
            next_offsets[bucket] += chunk[bucket];
        }
    }

    let dst_ptr = dst.as_mut_ptr() as usize;
    src.par_chunks(chunk_size)
        .enumerate()
        .for_each(|(chunk_idx, chunk)| {
            let mut offsets = chunked_offsets[chunk_idx];
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

pub fn par_radix_sort_single_pass_in<const BUCKETS: usize, const STRIDE: usize>(
    src: &[i32],
    dst: &mut [i32],
    total_counts: &mut [usize; BUCKETS],
    chunked_offsets: &mut Vec<usize>,
    chunked_counts: &mut Vec<usize>,
    chunk_size: usize,
    shift: usize,
) {
    if src.len() <= 1 {
        return;
    }
    debug_assert!(STRIDE >= BUCKETS);

    let mask = (BUCKETS as u32) - 1;
    let n_chunks = src.len().div_ceil(chunk_size);
    debug_assert!(chunked_counts.len() >= n_chunks * STRIDE);
    debug_assert!(chunked_offsets.len() >= n_chunks * STRIDE);

    chunked_counts
        .par_chunks_mut(STRIDE)
        .take(n_chunks)
        .zip(src.par_chunks(chunk_size))
        .for_each(|(counts_slot, chunk)| {
            let counts = &mut counts_slot[..BUCKETS];
            counts.fill(0);

            for &value in chunk {
                let key = ((value as u32) ^ 0x8000_0000) >> shift;
                let bucket = (key & mask) as usize;
                counts[bucket] += 1;
            }
        });

    total_counts
        .par_iter_mut()
        .enumerate()
        .for_each(|(bucket, total)| {
            *total = chunked_counts
                .chunks(STRIDE)
                .take(n_chunks)
                .map(|chunk| chunk[bucket])
                .sum();
        });

    let mut sum = 0;
    for i in 0..BUCKETS {
        let current = total_counts[i];
        total_counts[i] = sum;
        sum += current;
    }

    for (chunk, offset_chunk) in chunked_counts
        .chunks(STRIDE)
        .take(n_chunks)
        .zip(chunked_offsets.chunks_mut(STRIDE).take(n_chunks))
    {
        let chunk = &chunk[..BUCKETS];
        let offset_chunk = &mut offset_chunk[..BUCKETS];
        offset_chunk.copy_from_slice(total_counts);

        for bucket in 0..BUCKETS {
            total_counts[bucket] += chunk[bucket];
        }
    }

    let dst_ptr = dst.as_mut_ptr() as usize;
    src.par_chunks(chunk_size)
        .zip(chunked_offsets.par_chunks_mut(STRIDE).take(n_chunks))
        .for_each(|(chunk, offsets_slot)| {
            let offsets = &mut offsets_slot[..BUCKETS];
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

pub fn par_radix_sort_single_pass_in_u32<const BUCKETS: usize, const STRIDE: usize>(
    src: &[i32],
    dst: &mut [i32],
    total_counts: &mut [u32; BUCKETS],
    chunked_offsets: &mut Vec<u32>,
    chunked_counts: &mut Vec<u32>,
    chunk_size: usize,
    shift: usize,
) {
    if src.len() <= 1 {
        return;
    }
    assert!(src.len() <= u32::MAX as usize);
    debug_assert!(STRIDE >= BUCKETS);

    let mask = (BUCKETS as u32) - 1;
    let n_chunks = src.len().div_ceil(chunk_size);
    debug_assert!(chunked_counts.len() >= n_chunks * STRIDE);
    debug_assert!(chunked_offsets.len() >= n_chunks * STRIDE);

    chunked_counts
        .par_chunks_mut(STRIDE)
        .take(n_chunks)
        .zip(src.par_chunks(chunk_size))
        .for_each(|(counts_slot, chunk)| {
            let counts = &mut counts_slot[..BUCKETS];
            counts.fill(0);

            for &value in chunk {
                let key = ((value as u32) ^ 0x8000_0000) >> shift;
                let bucket = (key & mask) as usize;
                counts[bucket] += 1;
            }
        });

    total_counts
        .par_iter_mut()
        .enumerate()
        .for_each(|(bucket, total)| {
            *total = chunked_counts
                .chunks(STRIDE)
                .take(n_chunks)
                .map(|chunk| chunk[bucket])
                .sum();
        });

    let mut sum = 0;
    for i in 0..BUCKETS {
        let current = total_counts[i];
        total_counts[i] = sum;
        sum += current;
    }

    for (chunk, offset_chunk) in chunked_counts
        .chunks(STRIDE)
        .take(n_chunks)
        .zip(chunked_offsets.chunks_mut(STRIDE).take(n_chunks))
    {
        let chunk = &chunk[..BUCKETS];
        let offset_chunk = &mut offset_chunk[..BUCKETS];
        offset_chunk.copy_from_slice(total_counts);

        for bucket in 0..BUCKETS {
            total_counts[bucket] += chunk[bucket];
        }
    }

    let dst_ptr = dst.as_mut_ptr() as usize;
    src.par_chunks(chunk_size)
        .zip(chunked_offsets.par_chunks_mut(STRIDE).take(n_chunks))
        .for_each(|(chunk, offsets_slot)| {
            let offsets = &mut offsets_slot[..BUCKETS];
            for &value in chunk {
                let key = ((value as u32) ^ 0x8000_0000) >> shift;
                let bucket = (key & mask) as usize;
                let offset = offsets[bucket] as usize;
                unsafe {
                    (dst_ptr as *mut i32).add(offset).write(value);
                }
                offsets[bucket] += 1;
            }
        });
}

pub struct Radix11Scratch {
    dst: Vec<i32>,
    total_counts: [usize; RADIX_11_BUCKETS],
    chunked_offsets: Vec<usize>,
    chunked_counts: Vec<usize>,
}

pub struct Radix11ScratchU32 {
    dst: Vec<i32>,
    total_counts: [u32; RADIX_11_BUCKETS],
    chunked_offsets: Vec<u32>,
    chunked_counts: Vec<u32>,
}

impl Radix11ScratchU32 {
    pub fn new() -> Self {
        Self {
            dst: Vec::new(),
            total_counts: [0; RADIX_11_BUCKETS],
            chunked_offsets: Vec::new(),
            chunked_counts: Vec::new(),
        }
    }

    fn prepare(&mut self, len: usize, n_chunks: usize) {
        self.dst.resize(len, 0);
        self.chunked_offsets.resize(RADIX_11_BUCKETS * n_chunks, 0);
        self.chunked_counts.resize(RADIX_11_BUCKETS * n_chunks, 0);
    }
}

impl Default for Radix11ScratchU32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Radix11Scratch {
    pub fn new() -> Self {
        Self {
            dst: Vec::new(),
            total_counts: [0; RADIX_11_BUCKETS],
            chunked_offsets: Vec::new(),
            chunked_counts: Vec::new(),
        }
    }

    fn prepare(&mut self, len: usize, n_chunks: usize) {
        self.dst.resize(len, 0);
        self.chunked_offsets.resize(RADIX_11_BUCKETS * n_chunks, 0);
        self.chunked_counts.resize(RADIX_11_BUCKETS * n_chunks, 0);
    }
}

impl Default for Radix11Scratch {
    fn default() -> Self {
        Self::new()
    }
}

pub fn par_radix_sort_i32_11bit(values: &mut [i32]) {
    let mut scratch = Radix11Scratch::new();
    par_radix_sort_i32_11bit_with_scratch(values, &mut scratch);
}

pub fn par_radix_sort_i32_11bit_with_scratch(values: &mut [i32], scratch: &mut Radix11Scratch) {
    if values.len() <= 1 {
        return;
    }

    let n_chunks = rayon::current_num_threads();
    let chunk_size = values.len().div_ceil(n_chunks);
    scratch.prepare(values.len(), n_chunks);

    par_radix_sort_single_pass_in::<RADIX_11_BUCKETS, RADIX_11_BUCKETS>(
        values,
        &mut scratch.dst,
        &mut scratch.total_counts,
        &mut scratch.chunked_offsets,
        &mut scratch.chunked_counts,
        chunk_size,
        0,
    );
    par_radix_sort_single_pass_in::<RADIX_11_BUCKETS, RADIX_11_BUCKETS>(
        &scratch.dst,
        values,
        &mut scratch.total_counts,
        &mut scratch.chunked_offsets,
        &mut scratch.chunked_counts,
        chunk_size,
        11,
    );
    par_radix_sort_single_pass_in::<RADIX_10_BUCKETS, RADIX_11_BUCKETS>(
        &values,
        &mut scratch.dst,
        (&mut scratch.total_counts[..RADIX_10_BUCKETS])
            .try_into()
            .unwrap(),
        &mut scratch.chunked_offsets,
        &mut scratch.chunked_counts,
        chunk_size,
        22,
    );
    values.copy_from_slice(&scratch.dst);
}

pub fn par_radix_sort_i32_11bit_u32(values: &mut [i32]) {
    let mut scratch = Radix11ScratchU32::new();
    par_radix_sort_i32_11bit_u32_with_scratch(values, &mut scratch);
}

pub fn par_radix_sort_i32_11bit_u32_with_scratch(
    values: &mut [i32],
    scratch: &mut Radix11ScratchU32,
) {
    if values.len() <= 1 {
        return;
    }
    assert!(values.len() <= u32::MAX as usize);

    let n_chunks = rayon::current_num_threads();
    let chunk_size = values.len().div_ceil(n_chunks);
    scratch.prepare(values.len(), n_chunks);

    par_radix_sort_single_pass_in_u32::<RADIX_11_BUCKETS, RADIX_11_BUCKETS>(
        values,
        &mut scratch.dst,
        &mut scratch.total_counts,
        &mut scratch.chunked_offsets,
        &mut scratch.chunked_counts,
        chunk_size,
        0,
    );
    par_radix_sort_single_pass_in_u32::<RADIX_11_BUCKETS, RADIX_11_BUCKETS>(
        &scratch.dst,
        values,
        &mut scratch.total_counts,
        &mut scratch.chunked_offsets,
        &mut scratch.chunked_counts,
        chunk_size,
        11,
    );
    par_radix_sort_single_pass_in_u32::<RADIX_10_BUCKETS, RADIX_11_BUCKETS>(
        &values,
        &mut scratch.dst,
        (&mut scratch.total_counts[..RADIX_10_BUCKETS])
            .try_into()
            .unwrap(),
        &mut scratch.chunked_offsets,
        &mut scratch.chunked_counts,
        chunk_size,
        22,
    );
    values.copy_from_slice(&scratch.dst);
}

pub fn par_radix_sort_i32_9bit(values: &mut [i32]) {
    const BUCKETS_5: usize = 1 << 5;
    const BUCKETS_9: usize = 1 << 9;
    let chunk_size = values.len().div_ceil(rayon::current_num_threads());
    let mut dst = vec![0; values.len()];
    par_radix_sort_single_pass::<BUCKETS_9>(values, &mut dst, chunk_size, 0);
    par_radix_sort_single_pass::<BUCKETS_9>(&dst, values, chunk_size, 9);
    par_radix_sort_single_pass::<BUCKETS_9>(values, &mut dst, chunk_size, 18);
    par_radix_sort_single_pass::<BUCKETS_5>(&dst, values, chunk_size, 27);
}

pub fn par_radix_sort_i32_8bit2(values: &mut [i32]) {
    const BUCKETS_8: usize = 1 << 8;
    let chunk_size = values.len().div_ceil(rayon::current_num_threads());
    let mut dst = vec![0; values.len()];
    par_radix_sort_single_pass::<BUCKETS_8>(values, &mut dst, chunk_size, 0);
    par_radix_sort_single_pass::<BUCKETS_8>(&dst, values, chunk_size, 8);
    par_radix_sort_single_pass::<BUCKETS_8>(values, &mut dst, chunk_size, 16);
    par_radix_sort_single_pass::<BUCKETS_8>(&dst, values, chunk_size, 24);
}
pub fn par_radix_sort_i32_9_9_14bit(values: &mut [i32]) {
    const BUCKETS_9: usize = 1 << 9;
    const BUCKETS_14: usize = 1 << 14;
    let chunk_size = values.len().div_ceil(rayon::current_num_threads());
    let mut dst = vec![0; values.len()];
    par_radix_sort_single_pass::<BUCKETS_9>(values, &mut dst, chunk_size, 0);
    par_radix_sort_single_pass::<BUCKETS_9>(&dst, values, chunk_size, 9);
    par_radix_sort_single_pass::<BUCKETS_14>(values, &mut dst, chunk_size, 18);
    values.copy_from_slice(&dst);
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_par_radix_sort_8bit() {
        let v = Distribution::Random.values(100000);
        dbg!(&v);
        let mut v1 = v.clone();
        let mut v2 = v.clone();

        v1.sort_unstable();
        par_radix_sort_i32_8bit(&mut v2);

        assert_eq!(v1, v2)
    }
    #[test]
    fn test_par_radix_sort_11bit() {
        let v = Distribution::Random.values(10);
        dbg!(&v);
        let mut v1 = v.clone();
        let mut v2 = v.clone();

        v1.sort_unstable();
        par_radix_sort_i32_11bit(&mut v2);

        assert_eq!(v1, v2)
    }

    #[test]
    fn test_par_radix_sort_11bit_u32() {
        let v = Distribution::Random.values(100000);
        let mut v1 = v.clone();
        let mut v2 = v.clone();

        v1.sort_unstable();
        par_radix_sort_i32_11bit_u32(&mut v2);

        assert_eq!(v1, v2)
    }
}
