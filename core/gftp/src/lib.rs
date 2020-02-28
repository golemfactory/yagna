mod gftp;

const DEFAULT_CHUNK_SIZE : u64 = 40 * 1024;

pub use gftp::{download_file, Config};

pub async fn publish_file(dst_path: &std::path::Path) -> anyhow::Result<String> {
    Config {
        chunk_size: DEFAULT_CHUNK_SIZE
    }.publish(dst_path).await
}

