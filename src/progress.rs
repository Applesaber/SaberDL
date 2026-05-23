use indicatif::ProgressStyle;

pub fn worker_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{prefix:>6.cyan} {wide_bar:.cyan/blue} {percent:>3}% [{elapsed_precise}] {bytes:>10}/{total_bytes:>10}"
    ).unwrap()
}

pub fn total_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{prefix:>6.green.bold} {wide_bar:.green} {percent:>3}% [{elapsed_precise}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta})"
    ).unwrap()
}
