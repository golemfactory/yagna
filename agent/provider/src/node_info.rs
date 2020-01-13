use serde::{Serialize};



/// Temporary structures needed to make offer.
/// TODO: This should be moved somewhere else in the future.
#[derive(Serialize)]
pub struct CpuInfo {
    pub architecture: String,
    pub cores: u32,
    pub threads: u32,
}

#[derive(Serialize)]
pub struct NodeInfo {
    pub id: String,
    pub cpu: CpuInfo,
}

