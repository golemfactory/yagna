use sha2::digest::generic_array::GenericArray;
use sha2::Digest;
use std::fs::OpenOptions;
use std::io::Read;
use std::path::Path;

pub type HashOutput = GenericArray<u8, <sha3::Sha3_512 as Digest>::OutputSize>;

pub fn hash_file(path: &Path) -> HashOutput {
    let mut file_src = OpenOptions::new().read(true).open(path).expect("rnd file");

    let mut hasher = sha3::Sha3_512::default();
    let mut chunk = vec![0; 4096];

    while let Ok(count) = file_src.read(&mut chunk[..]) {
        hasher.input(&chunk[..count]);
        if count != 4096 {
            break;
        }
    }
    hasher.result()
}

pub fn verify_hash<S: AsRef<str>, P: AsRef<Path>>(hash: &HashOutput, path: P, file_name: S) {
    let path = path.as_ref().join(file_name.as_ref());
    log::info!("Verifying hash of {:?}", path);
    assert_eq!(hash, &hash_file(&path));
}
