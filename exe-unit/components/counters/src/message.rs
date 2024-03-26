use serde::{Deserialize, Serialize};

use crate::Result;
use actix::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Message)]
#[rtype(result = "Result<Vec<f64>>")]
pub struct GetMetrics;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "()")]
pub struct SetMetric {
    pub name: String,
    pub value: f64,
}

#[derive(Debug, Default, Message)]
#[rtype(result = "Result<()>")]
pub struct Shutdown;
