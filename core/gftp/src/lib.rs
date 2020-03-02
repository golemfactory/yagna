mod gftp;

use url::Url;
pub use gftp::{download_file, download_from_url, Config};


const DEFAULT_CHUNK_SIZE: u64 = 40 * 1024;


pub async fn publish_file(dst_path: &std::path::Path) -> anyhow::Result<Url> {
    Config {
        chunk_size: DEFAULT_CHUNK_SIZE,
    }
    .publish(dst_path)
    .await
}
