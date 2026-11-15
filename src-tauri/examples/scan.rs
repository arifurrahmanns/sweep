//! Runs a scan from the command line and prints the result as JSON.
//! It only reads the disk; nothing is deleted.
//!
//!     cargo run --release --example scan > scan.json

use sweep_lib::scanner::{scan, Env, ScanOptions};

fn main() {
    let env = Env::system();
    let started = std::time::Instant::now();
    let result = scan(&env, &ScanOptions::default(), &|step: &str| eprintln!("… {step}"));
    eprintln!("Scanned in {:.1}s", started.elapsed().as_secs_f64());
    println!("{}", serde_json::to_string_pretty(&result).expect("scan result is valid JSON"));
}
