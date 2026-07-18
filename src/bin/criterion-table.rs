use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

const PREFERRED_ALGORITHM_ORDER: &[&str] = &[
    "sort_unstable",
    "par_sort_unstable",
    "par_radix_sort_i32_8bit",
    "par_radix_sort_i32_11bit",
    "par_radix_sort_i32_11bit_scratch",
    "par_radix_sort_i32_11bit_u32",
    "par_radix_sort_i32_11bit_u32_scratch",
];

fn main() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut args = vec![
        "bench".to_string(),
        "--bench".to_string(),
        "sort".to_string(),
        "--".to_string(),
        "--quiet".to_string(),
        "--format".to_string(),
        "terse".to_string(),
        "--discard-baseline".to_string(),
        "--noplot".to_string(),
    ];
    args.extend(std::env::args().skip(1));

    let output = Command::new(cargo)
        .args(args)
        .output()
        .expect("failed to run criterion benchmark");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        eprint!("{stderr}");
        eprint!("{stdout}");
        std::process::exit(output.status.code().unwrap_or(1));
    }

    let mut current_benchmark = None;
    let mut tables = BTreeMap::<String, BTreeMap<usize, BTreeMap<String, String>>>::new();

    for line in stdout.lines().chain(stderr.lines()) {
        let trimmed = line.trim();

        if trimmed.starts_with("sort/") {
            current_benchmark = parse_benchmark_name(trimmed);
            continue;
        }

        if let Some(time) = trimmed.strip_prefix("time:").and_then(parse_time) {
            if let Some((distribution, size, algorithm)) = current_benchmark.take() {
                tables
                    .entry(distribution)
                    .or_default()
                    .entry(size)
                    .or_default()
                    .insert(algorithm, time);
            }
        }
    }

    for (distribution, rows) in tables {
        print_table(&distribution, &rows);
    }
}

fn parse_benchmark_name(name: &str) -> Option<(String, usize, String)> {
    let mut parts = name.split('/');
    let sort = parts.next()?;
    let distribution = parts.next()?;
    let size = parts.next()?.parse().ok()?;
    let algorithm = parts.next()?;

    if sort == "sort" && parts.next().is_none() {
        Some((distribution.to_string(), size, algorithm.to_string()))
    } else {
        None
    }
}

fn parse_time(line: &str) -> Option<String> {
    let estimates = line.split_once('[')?.1.split_once(']')?.0;
    let mut tokens = estimates.split_whitespace();
    tokens.next()?;
    tokens.next()?;
    let median_value = tokens.next()?;
    let median_unit = tokens.next()?;
    Some(format!("{median_value} {median_unit}"))
}

fn print_table(distribution: &str, rows: &BTreeMap<usize, BTreeMap<String, String>>) {
    let mut discovered = rows
        .values()
        .flat_map(|row| row.keys().cloned())
        .collect::<BTreeSet<_>>();
    let mut algorithms = Vec::new();

    for algorithm in PREFERRED_ALGORITHM_ORDER {
        if discovered.remove(*algorithm) {
            algorithms.push((*algorithm).to_string());
        }
    }

    algorithms.extend(discovered);

    let mut widths = Vec::with_capacity(algorithms.len() + 1);
    widths.push(
        rows.keys()
            .map(|size| size.to_string().len())
            .max()
            .unwrap_or(4)
            .max("size".len()),
    );

    for algorithm in &algorithms {
        let value_width = rows
            .values()
            .filter_map(|row| row.get(algorithm))
            .map(|value| value.len())
            .max()
            .unwrap_or(0);
        widths.push(algorithm.len().max(value_width));
    }

    println!("sort/{distribution}");
    print!("{:>width$}", "size", width = widths[0]);
    for (algorithm, width) in algorithms.iter().zip(&widths[1..]) {
        print!("  {:>width$}", algorithm, width = width);
    }
    println!();

    for (size, row) in rows {
        print!("{:>width$}", size, width = widths[0]);
        for (algorithm, width) in algorithms.iter().zip(&widths[1..]) {
            let value = row.get(algorithm).map(String::as_str).unwrap_or("-");
            print!("  {:>width$}", value, width = width);
        }
        println!();
    }
}
