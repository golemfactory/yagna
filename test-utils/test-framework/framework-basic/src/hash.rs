use digest::{Digest, Output};
use std::fs::OpenOptions;
use std::io::Read;
use std::path::Path;

pub fn hash_file<H: Digest>(path: &Path) -> Output<H> {
    let mut file_src = OpenOptions::new().read(true).open(path).expect("rnd file");

    let mut hasher = H::new();
    let mut chunk = vec![0; 4096];

    while let Ok(count) = file_src.read(&mut chunk[..]) {
        hasher.update(&chunk[..count]);
        if count != 4096 {
            break;
        }
    }
    hasher.finalize()
}

pub fn verify_hash<H>(hash: &Output<H>, path: impl AsRef<Path>, file_name: impl AsRef<str>)
where
    H: Digest,
{
    let path = path.as_ref().join(file_name.as_ref());
    log::info!("Verifying hash of {:?}", path);
    assert_eq!(hash, &hash_file::<H>(&path));
}
