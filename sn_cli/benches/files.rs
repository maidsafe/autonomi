use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process::{exit, Command};
use std::time::Duration;
use tempfile::tempdir;

const SAMPLE_SIZE: usize = 50;

// This procedure includes the client startup, which will be measured by criterion as well.
// As normal user won't care much about initial client startup,
// but be more alerted on communication speed during transmission.
// It will be better to execute bench test with `local-discovery`,
// to make the measurement results reflect speed improvement or regression more accurately.
fn safe_files_upload(dir: &str) {
    let output = Command::new("./target/release/safe")
        .arg("files")
        .arg("upload")
        .arg(dir)
        .output()
        .expect("Failed to execute command");

    if !output.status.success() {
        panic!("Upload command executed with failing error code");
    }
}

fn safe_files_download() {
    let output = Command::new("./target/release/safe")
        .arg("files")
        .arg("download")
        .output()
        .expect("Failed to execute command");

    if !output.status.success() {
        panic!("Download command executed with failing error code");
    }
}

fn create_file(size_mb: u64) -> tempfile::TempDir {
    let dir = tempdir().expect("Failed to create temporary directory");
    let file_path = dir.path().join("tempfile");

    let mut file = File::create(file_path).expect("Failed to create file");
    let data = vec![0u8; (size_mb * 1024 * 1024) as usize]; // Create a vector with size_mb MB of data
    file.write_all(&data).expect("Failed to write to file");

    dir
}

fn fund_cli_wallet() {
    let output = Command::new("./target/release/safe")
        .arg("wallet")
        .arg("address")
        .output()
        .expect("Failed to execute 'safe wallet address' command");

    let str = String::from_utf8(output.stdout).unwrap();
    let addr = str.lines().last().unwrap();

    let _ = Command::new("./target/release/faucet")
        .arg("claim-genesis")
        .output()
        .expect("Failed to execute 'faucet claim-genesis");

    let output = Command::new("./target/release/faucet")
        .arg("send")
        .arg("10000")
        .arg(addr)
        .output()
        .expect("Failed to execute 'faucet send 10000' command");

    let str = String::from_utf8(output.stdout).unwrap();
    let dbc_hex = str.lines().last().unwrap();

    let _ = Command::new("./target/release/safe")
        .arg("wallet")
        .arg("deposit")
        .arg("--dbc")
        .arg(dbc_hex)
        .output()
        .expect("Failed to execute 'safe wallet deposit' command");
}

fn criterion_benchmark(c: &mut Criterion) {
    // Check if the binary exists
    if !Path::new("./target/release/safe").exists() {
        eprintln!("Error: Binary ./target/release/safe does not exist. Please make sure to compile your project first");
        exit(1);
    }

    let sizes = vec![1, 10]; // File sizes in MB. Add more sizes as needed

    for size in sizes.iter() {
        let dir = create_file(*size);
        let dir_path = dir.path().to_str().unwrap();
        fund_cli_wallet();

        let mut group = c.benchmark_group(format!("Upload Benchmark {}MB", size));
        group.sampling_mode(criterion::SamplingMode::Flat);
        group.measurement_time(Duration::from_secs(120));
        group.warm_up_time(Duration::from_secs(5));
        group.sample_size(SAMPLE_SIZE);

        // Set the throughput to be reported in terms of bytes
        group.throughput(Throughput::Bytes(size * 1024 * 1024));
        let bench_id = format!("safe files upload {}mb", size);
        group.bench_function(bench_id, |b| b.iter(|| safe_files_upload(dir_path)));
        group.finish();
    }

    let mut group = c.benchmark_group("Download Benchmark".to_string());
    group.sampling_mode(criterion::SamplingMode::Flat);
    group.measurement_time(Duration::from_secs(120));
    group.warm_up_time(Duration::from_secs(5));

    // The download will download all uploaded files during bench.
    // If the previous bench executed with the default 100 sample size,
    // there will then be around 1.1GB in total, and may take around 40s for each iteratioin.
    // Hence we have to reduce the number of iterations from the default 100 to 10,
    // To avoid the benchmark test taking over one hour to complete.
    let total_size: u64 = sizes.iter().map(|size| SAMPLE_SIZE as u64 * size).sum();
    group.sample_size(SAMPLE_SIZE / 5);

    // Set the throughput to be reported in terms of bytes
    group.throughput(Throughput::Bytes(total_size * 1024 * 1024));
    let bench_id = "safe files download".to_string();
    group.bench_function(bench_id, |b| b.iter(safe_files_download));
    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
