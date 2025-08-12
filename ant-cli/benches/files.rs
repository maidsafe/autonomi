// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// Allow expect/panic usage in benchmarks
#![allow(clippy::expect_used)]
#![allow(clippy::panic)]

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use rand::{Rng, thread_rng};
use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use std::{
    collections::HashSet,
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, exit},
    time::Duration,
};
use tempfile::tempdir;

const SAMPLE_SIZE: usize = 20;

// Default deployer wallet of the testnet.
const DEFAULT_WALLET_PRIVATE_KEY: &str =
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

// This procedure includes the client startup, which will be measured by criterion as well.
// As normal user won't care much about initial client startup,
// but be more alerted on communication speed during transmission.
// It will be better to execute bench test with `local`,
// to make the measurement results reflect speed improvement or regression more accurately.
fn autonomi_file_upload(dir: &str) -> String {
    let autonomi_cli_path = get_cli_path();
    let output = Command::new(autonomi_cli_path)
        .arg("--local")
        .arg("file")
        .arg("upload")
        .arg(dir)
        .output()
        .expect("Failed to execute command");

    if !output.status.success() {
        let err = output.stderr;
        let err_string = String::from_utf8(err).expect("Failed to parse error string");
        panic!("Upload command executed with failing error code: {err_string:?}");
    } else {
        let out = output.stdout;
        let out_string = String::from_utf8(out).expect("Failed to parse output string");
        println!("upload output is :\n{out_string:?}");

        // Debug: print all lines to see what we're working with
        println!("All lines in output:");
        for (i, line) in out_string.lines().enumerate() {
            println!("Line {i}: {line:?}");
            if line.contains("At address:") {
                println!("Found line with 'At address:' at index {i}");
            }
        }

        let address = out_string
            .lines()
            .find(|line| line.contains("At address:"))
            .expect("Failed to find the address of the uploaded file");

        // Extract the address from the line
        let address = address
            .split("At address:")
            .nth(1)
            .expect("Failed to extract address from line")
            .trim();

        let address_str = address.to_string();
        println!("Parsed address is: {address_str:?}");
        address_str
    }
}

fn autonomi_file_download(uploaded_files: HashSet<String>) {
    let autonomi_cli_path = get_cli_path();

    let temp_dir = tempdir().expect("Failed to create temp dest dir");
    for address in uploaded_files.iter() {
        let dest_file = temp_dir.path().join(address);

        println!("Trying to download {address:?} to as the dest_file of {dest_file:?}");

        let output = Command::new(autonomi_cli_path.clone())
            .arg("--local")
            .arg("file")
            .arg("download")
            .arg("--disable-cache")
            .arg(address)
            .arg(dest_file)
            .output()
            .expect("Failed to execute command");

        if !output.status.success() {
            let err = output.stderr;
            let err_string = String::from_utf8(err).expect("Failed to parse error string");
            panic!("Download command executed with failing error code: {err_string:?}");
        }
    }
}

fn generate_file(path: &PathBuf, file_size_mb: usize) {
    let mut file = File::create(path).expect("Failed to create file");
    let mut rng = thread_rng();

    // can create [u8; 32] max at time. Thus each mb has 1024*32 such small chunks
    let n_small_chunks = file_size_mb * 1024 * 32;
    for _ in 0..n_small_chunks {
        let random_data: [u8; 32] = rng.r#gen();
        file.write_all(&random_data)
            .expect("Failed to write to file");
    }
    let size = file.metadata().expect("Failed to get metadata").len() as f64 / (1024 * 1024) as f64;
    assert_eq!(file_size_mb as f64, size);
}

fn get_cli_path() -> PathBuf {
    let mut path = PathBuf::new();
    if let Ok(val) = std::env::var("CARGO_TARGET_DIR") {
        path.push(val);
    } else {
        path.push("target");
    }
    path.push("release");
    path.push("ant");
    path
}

fn criterion_benchmark(c: &mut Criterion) {
    // Check if the binary exists
    let cli_path = get_cli_path();
    if !Path::new(&cli_path).exists() {
        eprintln!(
            "Error: Binary {cli_path:?} does not exist. Please make sure to compile your project first"
        );
        exit(1);
    }

    if std::env::var("SECRET_KEY").is_err() {
        // SAFETY: This is called during benchmark initialization before any other threads
        // are spawned, so there's no risk of data races. Setting SECRET_KEY is necessary
        // for benchmark execution.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("SECRET_KEY", DEFAULT_WALLET_PRIVATE_KEY);
        }
    }

    let sizes: [u64; 2] = [1, 10]; // File sizes in MB. Add more sizes as needed
    let mut total_uploaded_files = HashSet::new();
    let mut total_size: u64 = 0;

    for size in sizes.iter() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let temp_dir_path = temp_dir.keep();
        let mut uploaded_files = HashSet::new();

        // create 23 random files. This is to keep the benchmark results consistent with prior runs. The change to make
        // use of ChunkManager means that we don't upload the same file twice and the `uploaded_files` file is now read
        // as a set and we don't download the same file twice. Hence create 23 files as counted from the logs
        // pre ChunkManager change.
        //
        // With the client performance improved, more random_files need to be generated to avoid
        // upload same content that pollutes the tests.
        let file_paths: Vec<PathBuf> = (0..50)
            .into_par_iter()
            .map(|idx| {
                let path = temp_dir_path.join(format!("random_file_{size}_mb_{idx}"));
                generate_file(&path, *size as usize);
                path
            })
            .collect();

        // Wait little bit for the fund to be settled.
        std::thread::sleep(Duration::from_secs(10));

        let mut group = c.benchmark_group(format!("Upload Benchmark {size}MB"));
        group.sampling_mode(criterion::SamplingMode::Flat);
        // One sample may compose of multiple iterations, and this is decided by `measurement_time`.
        // Set this to a lower value to ensure each sample only contains one iteration.
        // To ensure the download throughput calculation is correct.
        group.measurement_time(Duration::from_secs(5));
        group.warm_up_time(Duration::from_secs(5));
        group.sample_size(SAMPLE_SIZE);

        // Create an iterator. Shall not use `cycle` to avoid upload duplicated content.
        let mut file_path_iter = file_paths.iter();

        // Set the throughput to be reported in terms of bytes
        group.throughput(Throughput::Bytes(size * 1024 * 1024));
        let bench_id = format!("ant files upload {size}mb");
        group.bench_function(bench_id, |b| {
            b.iter(|| {
                let file_path = file_path_iter.next().expect("Temp files drained up.");
                let uploaded_address = autonomi_file_upload(
                    file_path
                        .to_str()
                        .expect("Invalid temp file path encountered"),
                );
                uploaded_files.insert(uploaded_address);
            })
        });
        group.finish();
        println!(
            "Got total {} files uploaded after iteration of {size} data_size.",
            uploaded_files.len()
        );

        // During `measurement_time` and `warm_up_time`, there will be one upload run for each.
        // Which means two additional `uploaded_files` created and for downloading.
        total_size += size * (uploaded_files.len() as u64 + 2);

        total_uploaded_files.extend(uploaded_files);
    }

    let mut group = c.benchmark_group("Download Benchmark".to_string());
    group.sampling_mode(criterion::SamplingMode::Flat);
    group.measurement_time(Duration::from_secs(10));
    group.warm_up_time(Duration::from_secs(5));

    // The download will download all uploaded files during bench.
    // If the previous bench executed with the default 100 sample size,
    // there will then be around 1.1GB in total, and may take around 40s for each iteratioin.
    // Hence we have to reduce the number of iterations from the default 100 to 10,
    // To avoid the benchmark test taking over one hour to complete.
    group.sample_size(SAMPLE_SIZE / 2);

    // Set the throughput to be reported in terms of bytes
    group.throughput(Throughput::Bytes(total_size * 1024 * 1024));
    let bench_id = "ant files download".to_string();
    group.bench_function(bench_id, |b| {
        b.iter(|| autonomi_file_download(total_uploaded_files.clone()))
    });
    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
