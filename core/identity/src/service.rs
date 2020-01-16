use futures::lock::Mutex;
use futures::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
/// Identity service
use ya_core_model::ethaddr::NodeId;
use ya_core_model::identity::IdentityInfo;
use ya_service_bus::actix_rpc::bind;

use crate::dao::appkey::DaoError;
use crate::dao::identity::IdentityDao;
use crate::db::models::Identity;
use chrono::Utc;
use ethsign::KeyFile;
use std::convert::{identity, TryInto};
use ya_persistence::executor::DbExecutor;

mod appkey;
mod identity;

pub async fn activate(db: &DbExecutor) -> anyhow::Result<()> {
    log::info!("activating identity service");
    log::debug!("loading default identity");

    let service = Arc::new(Mutex::new(
        identity::IdentityService::from_db(db.clone()).await?,
    ));
    identity::IdentityService::bind_service(service);
    log::info!("identity service activated");

    appkey::activate(db).await?;
    Ok(())
}
