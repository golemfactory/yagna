use metrics::counter;

use ya_core_model::misc;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::{typed as bus, RpcMessage};


pub type RpcMessageResult<T> = Result<<T as RpcMessage>::Item, <T as RpcMessage>::Error>;

pub fn bind_gsb(db: &DbExecutor) {
    bus::ServiceBinder::new(misc::BUS_ID, db, ())
        .bind(get_misc_gsb);

    // Initialize counters to 0 value. Otherwise they won't appear on metrics endpoint
    // until first change to value will be made.
    counter!("version.new", 0);
    counter!("version.skip", 0);
}



async fn get_misc_gsb(
    db: DbExecutor,
    _caller: String,
    msg: misc::Get,
) -> RpcMessageResult<misc::Get> {
    Ok(misc::MiscInfo {test: "testing gsb misc".to_string()})


}
