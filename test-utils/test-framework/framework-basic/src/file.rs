use rand::Rng;
use sha2::Digest;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::hash::HashOutput;

pub fn generate_file_with_hash(
    path: &Path,
    name: &str,
    chunk_size: usize,
    chunk_count: usize,
) -> HashOutput {
    let path = path.join(name);
    let mut hasher = sha3::Sha3_512::default();
    let mut file_src = OpenOptions::new()
        .write(true)
        .create(true)
        .open(path)
        .expect("rnd file");

    let mut rng = rand::thread_rng();

    for _ in 0..chunk_count {
        let input: Vec<u8> = (0..chunk_size)
            .map(|_| rng.gen_range(0..256) as u8)
            .collect();

        hasher.input(&input);
        let _ = file_src.write(&input).unwrap();
    }
    file_src.flush().unwrap();
    hasher.result()
}

pub fn generate_file(path: &PathBuf, chunk_size: usize, chunk_count: usize) {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .open(path)
        .expect("rnd file");

    let mut rng = rand::thread_rng();
    let input: Vec<u8> = (0..chunk_size)
        .map(|_| rng.gen_range(0..256) as u8)
        .collect();

    for _ in 0..chunk_count {
        let _ = file.write(&input).unwrap();
    }
    file.flush().unwrap();
}
