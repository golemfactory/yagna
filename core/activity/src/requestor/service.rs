use ya_core_model::activity::{self, RuntimeEvent, RuntimeEventKind};
use ya_persistence::executor::DbExecutor;
use ya_service_bus::{timeout::*, typed::ServiceBinder};

use crate::common::{
    authorize_activity_initiator, RpcMessageResult,
};
use crate::db::models::RuntimeEventType;

pub fn bind_gsb(db: &DbExecutor) {
    ServiceBinder::new(activity::BUS_ID, db, ())
        .bind(receive_runtime_event_gsb);
}

async fn receive_runtime_event_gsb(
    db: DbExecutor,
    caller: String,
    msg: activity::ReceiveRuntimeEvent,
) -> RpcMessageResult<activity::ReceiveRuntimeEvent> {
    authorize_activity_initiator(&db, caller, &msg.activity_id).await?;

    // TODO
    Ok(())
}
