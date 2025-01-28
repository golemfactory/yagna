use digest::{Digest, Output};
use rand::rngs::ThreadRng;
use rand::Rng;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

trait ContentGenerator {
    fn generate(&mut self, chunk_size: usize) -> Vec<u8>;
}

struct RandomGenerator(pub ThreadRng);
struct ZeroGenerator {}

impl ContentGenerator for RandomGenerator {
    fn generate(&mut self, chunk_size: usize) -> Vec<u8> {
        (0..chunk_size)
            .map(|_| self.0.gen_range(0..256) as u8)
            .collect()
    }
}

impl ContentGenerator for ZeroGenerator {
    fn generate(&mut self, chunk_size: usize) -> Vec<u8> {
        vec![0; chunk_size]
    }
}

pub fn generate_random_file_with_hash<H: Digest>(
    path: &Path,
    name: &str,
    chunk_size: usize,
    chunk_count: usize,
) -> Output<H> {
    generate_file_with_hasher::<H>(
        path,
        name,
        chunk_size,
        chunk_count,
        RandomGenerator(rand::thread_rng()),
    )
}

pub fn generate_file_with_hash(
    path: &Path,
    name: &str,
    chunk_size: usize,
    chunk_count: usize,
) -> Output<sha3::Sha3_512> {
    generate_random_file_with_hash::<sha3::Sha3_512>(path, name, chunk_size, chunk_count)
}

pub fn generate_image(
    path: &Path,
    name: &str,
    chunk_size: usize,
    chunk_count: usize,
) -> Output<sha3::Sha3_512> {
    generate_file_with_hasher::<sha3::Sha3_512>(
        path,
        name,
        chunk_size,
        chunk_count,
        ZeroGenerator {},
    )
}

fn generate_file_with_hasher<H: Digest>(
    path: &Path,
    name: &str,
    chunk_size: usize,
    chunk_count: usize,
    mut gen: impl ContentGenerator,
) -> Output<H> {
    fs::create_dir_all(path).ok();
    let path = path.join(name);

    log::debug!(
        "Creating a random file {} of size {chunk_size} * {chunk_count}",
        path.display()
    );
    let mut hasher = H::new();
    let mut file_src = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .expect("rnd file");

    for i in 0..chunk_count {
        log::trace!(
            "Generating chunk {i}/{chunk_count}. File size: {}/{}",
            i * chunk_size,
            chunk_count * chunk_size
        );

        let input: Vec<u8> = gen.generate(chunk_size);

        hasher.update(&input);
        let _ = file_src.write(&input).unwrap();
    }
    file_src.flush().unwrap();
    hasher.finalize()
}

pub fn generate_file(path: &PathBuf, chunk_size: usize, chunk_count: usize) {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
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
