use futures::prelude::*;
use std::convert::From;

use crate::common::{
    authorize_activity_initiator, authorize_agreement_initiator, generate_id, get_agreement,
    get_persisted_state, get_persisted_usage, RpcMessageResult,
};
use crate::dao::*;
use crate::error::Error;
use ya_core_model::activity;
use ya_model::activity::State;
use ya_persistence::executor::DbExecutor;
use ya_persistence::models::ActivityEventType;
use ya_service_bus::{timeout::*, typed as bus, RpcMessage};

struct ServiceBinder<'a, 'b> {
    bus_addr: &'b str,
    db: &'a DbExecutor,
}

impl<'a, 'b> ServiceBinder<'a, 'b> {
    fn bind<F: 'static, Msg: RpcMessage, Output: 'static>(self, f: F) -> Self
    where
        F: Fn(DbExecutor, String, Msg) -> Output,
        Output: Future<Output = std::result::Result<Msg::Item, Msg::Error>>,
        Msg::Error: std::fmt::Debug,
    {
        let db = self.db.clone();
        let _ = bus::bind_with_caller(self.bus_addr, move |addr, msg| {
            log::debug!("Received call to {}", Msg::ID);
            let fut = f(db.clone(), addr, msg);
            fut.map(|res| {
                match &res {
                    Ok(_) => log::debug!("Call to {} successful", Msg::ID),
                    Err(e) => log::debug!("Call to {} failed: {:?}", Msg::ID, e),
                }
                res
            })
        });
        self
    }
}

pub fn bind_gsb(db: &DbExecutor) {
    // public for remote requestors interactions
    let _ = ServiceBinder {
        bus_addr: activity::BUS_ID,
        db,
    }
    .bind(create_activity_gsb)
    .bind(destroy_activity_gsb)
    .bind(get_activity_state_gsb)
    .bind(get_activity_usage_gsb);

    local::bind_gsb(db);
}

/// Creates new Activity based on given Agreement.
async fn create_activity_gsb(
    db: DbExecutor,
    caller: String,
    msg: activity::Create,
) -> RpcMessageResult<activity::Create> {
    authorize_agreement_initiator(caller, &msg.agreement_id).await?;

    let activity_id = generate_id();
    let provider_id = get_agreement(&msg.agreement_id)
        .await?
        .offer
        .provider_id
        .ok_or(Error::BadRequest("Invalid agreement".to_owned()))?;

    db.as_dao::<ActivityDao>()
        .create_if_not_exists(&activity_id, &msg.agreement_id)
        .await
        .map_err(Error::from)?;

    log::debug!("activity inserted: {}", activity_id);

    db.as_dao::<EventDao>()
        .create(
            &activity_id,
            &provider_id,
            ActivityEventType::CreateActivity,
        )
        .await
        .map_err(Error::from)?;

    let state = db
        .as_dao::<ActivityStateDao>()
        .get_state_wait(&activity_id, None)
        .timeout(msg.timeout)
        .map_err(Error::from)
        .await?
        .map_err(Error::from)?;
    log::debug!("activity state: {:?}", state);

    Ok(activity_id)
}

/// Destroys given Activity.
async fn destroy_activity_gsb(
    db: DbExecutor,
    caller: String,
    msg: activity::Destroy,
) -> RpcMessageResult<activity::Destroy> {
    authorize_activity_initiator(&db, caller, &msg.activity_id).await?;
    let provider_id = get_agreement(&msg.agreement_id)
        .await?
        .offer
        .provider_id
        .ok_or(Error::BadRequest("Invalid agreement".to_owned()))?;

    db.as_dao::<EventDao>()
        .create(
            &msg.activity_id,
            &provider_id,
            ActivityEventType::DestroyActivity,
        )
        .await
        .map_err(Error::from)?;

    log::debug!(
        "waiting {:?}ms for activity status change to Terminate",
        msg.timeout
    );
    Ok(db
        .as_dao::<ActivityStateDao>()
        .get_state_wait(&msg.activity_id, Some(State::Terminated.into()))
        .timeout(msg.timeout)
        .map_err(Error::from)
        .await
        .map(|_| ())?)
}

async fn get_activity_state_gsb(
    db: DbExecutor,
    caller: String,
    msg: activity::GetState,
) -> RpcMessageResult<activity::GetState> {
    authorize_activity_initiator(&db, caller, &msg.activity_id).await?;

    Ok(get_persisted_state(&db, &msg.activity_id).await?)
}

async fn get_activity_usage_gsb(
    db: DbExecutor,
    caller: String,
    msg: activity::GetUsage,
) -> RpcMessageResult<activity::GetUsage> {
    authorize_activity_initiator(&db, caller, &msg.activity_id).await?;

    Ok(get_persisted_usage(&db, &msg.activity_id).await?)
}

mod local {
    use super::*;
    use crate::common::{set_persisted_state, set_persisted_usage};

    pub fn bind_gsb(db: &DbExecutor) {
        // local for ExeUnit interactions
        let _ = ServiceBinder {
            bus_addr: activity::local::BUS_ID,
            db,
        }
        .bind(set_activity_state_gsb)
        .bind(set_activity_usage_gsb);
    }

    /// Pass activity state (which may include error details).
    /// Called by ExeUnits.
    async fn set_activity_state_gsb(
        db: DbExecutor,
        caller: String,
        msg: activity::local::SetState,
    ) -> RpcMessageResult<activity::local::SetState> {
        authorize_activity_initiator(&db, caller, &msg.activity_id).await?;

        set_persisted_state(&db, &msg.activity_id, msg.state).await?;
        Ok(())
    }

    /// Pass current activity usage (which may include error details).
    /// Called by ExeUnits.
    async fn set_activity_usage_gsb(
        db: DbExecutor,
        caller: String,
        msg: activity::local::SetUsage,
    ) -> RpcMessageResult<activity::local::SetUsage> {
        authorize_activity_initiator(&db, caller, &msg.activity_id).await?;

        set_persisted_usage(&db, &msg.activity_id, msg.usage).await?;
        Ok(())
    }
}
