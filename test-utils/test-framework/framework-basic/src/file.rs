use rand::Rng;
use sha2::Digest;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use crate::hash::HashOutput;

pub fn create_file(path: &Path, name: &str, chunk_size: usize, chunk_count: usize) -> HashOutput {
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
