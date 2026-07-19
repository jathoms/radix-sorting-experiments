use sorting_benchmarks::msd::sort::{par_msd_radix_sort_8bit_new, par_sort_unstable_new};
use sorting_benchmarks::{
    Distribution, Radix11Scratch, par_radix_sort_i32_11bit, par_radix_sort_i32_11bit_with_scratch,
    par_sort_unstable,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static TOTAL_ALLOCATED: AtomicUsize = AtomicUsize::new(0);

const SIZES: &[usize] = &[100_000, 1_000_000, 5_000_000];

struct CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record_alloc(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        CURRENT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.saturating_sub(layout.size()))
            })
            .ok();
    }

    unsafe fn realloc(&self, ptr: *mut u8, old_layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, old_layout, new_size) };
        if !new_ptr.is_null() {
            if new_size > old_layout.size() {
                record_alloc(new_size - old_layout.size());
            } else {
                let freed = old_layout.size() - new_size;
                CURRENT
                    .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                        Some(current.saturating_sub(freed))
                    })
                    .ok();
            }
        }
        new_ptr
    }
}

fn main() {
    warm_up_rayon();

    println!("algorithm\tsize\ttotal_allocated\tpeak_live");

    for &size in SIZES {
        let input = Distribution::Random.values(size);

        measure_mut("par_sort_unstable_inplace", size, &input, par_sort_unstable);
        measure_new("par_sort_unstable_new", size, &input, par_sort_unstable_new);
        measure_new("par_msd_new", size, &input, par_msd_radix_sort_8bit_new);
        measure_mut(
            "par_lsd_11bit_one_shot",
            size,
            &input,
            par_radix_sort_i32_11bit,
        );
        measure_lsd_scratch(size, &input);
    }
}

fn measure_mut(name: &str, size: usize, input: &[i32], sort: fn(&mut [i32])) {
    let mut warmup = input.to_vec();
    sort(&mut warmup);

    let mut values = input.to_vec();
    reset_counts();
    sort(&mut values);
    black_box(values);
    print_measurement(name, size, snapshot());
}

fn measure_new(name: &str, size: usize, input: &[i32], sort: fn(&mut [i32]) -> Vec<i32>) {
    let mut warmup = input.to_vec();
    black_box(sort(&mut warmup));

    let mut values = input.to_vec();
    reset_counts();
    let sorted = sort(&mut values);
    black_box(sorted);
    black_box(values);
    print_measurement(name, size, snapshot());
}

fn measure_lsd_scratch(size: usize, input: &[i32]) {
    let mut scratch = Radix11Scratch::new();
    let mut warmup = input.to_vec();
    par_radix_sort_i32_11bit_with_scratch(&mut warmup, &mut scratch);

    let mut values = input.to_vec();
    reset_counts();
    par_radix_sort_i32_11bit_with_scratch(&mut values, &mut scratch);
    black_box(values);
    black_box(scratch);
    print_measurement("par_lsd_11bit_reused_scratch", size, snapshot());
}

fn warm_up_rayon() {
    let mut values = Distribution::Random.values(10_000);
    par_sort_unstable(&mut values);
}

fn record_alloc(size: usize) {
    TOTAL_ALLOCATED.fetch_add(size, Ordering::Relaxed);
    let current = CURRENT.fetch_add(size, Ordering::Relaxed) + size;
    let mut peak = PEAK.load(Ordering::Relaxed);
    while current > peak {
        match PEAK.compare_exchange_weak(peak, current, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(next_peak) => peak = next_peak,
        }
    }
}

fn reset_counts() {
    CURRENT.store(0, Ordering::Relaxed);
    PEAK.store(0, Ordering::Relaxed);
    TOTAL_ALLOCATED.store(0, Ordering::Relaxed);
}

fn snapshot() -> (usize, usize) {
    (
        TOTAL_ALLOCATED.load(Ordering::Relaxed),
        PEAK.load(Ordering::Relaxed),
    )
}

fn print_measurement(name: &str, size: usize, (total_allocated, peak_live): (usize, usize)) {
    println!(
        "{name}\t{size}\t{}\t{}",
        format_bytes(total_allocated),
        format_bytes(peak_live)
    );
}

fn format_bytes(bytes: usize) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * KIB;

    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / KIB)
    } else {
        format!("{:.1} MiB", bytes as f64 / MIB)
    }
}
