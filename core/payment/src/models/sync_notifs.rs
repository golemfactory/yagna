use crate::schema::pay_sync_needed_notifs;
use chrono::NaiveDateTime;
use ya_client_model::NodeId;

#[derive(Queryable, Debug, Identifiable, Insertable, AsChangeset)]
#[table_name = "pay_sync_needed_notifs"]
pub struct WriteObj {
    pub id: NodeId,
    pub timestamp: Option<NaiveDateTime>,
    pub retries: Option<i32>,
}

impl WriteObj {
    pub fn new(id: NodeId) -> Self {
        WriteObj {
            id,
            timestamp: None,
            retries: None,
        }
    }

    pub fn from_read(read: ReadObj) -> Self {
        WriteObj {
            id: read.id,
            timestamp: Some(read.timestamp),
            retries: Some(read.retries),
        }
    }
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "pay_sync_needed_notifs"]
pub struct ReadObj {
    pub id: NodeId,
    pub timestamp: NaiveDateTime,
    pub retries: i32,
}
