use std::time::SystemTime;
use serde::{Deserialize, Serialize};
use ya_client_model::NodeId;

type CallId = u64;

#[derive(Serialize, Deserialize)]
enum RpcMessage {
    Ping {
        id : u64,
        ts : u64,
    },
    PingResp {
        id : u64,
        ts_in : u64,
        ts_out : u64,
    },
    Call {
        call_id: CallId,
        caller : NodeId,
        service : String,
        body : Vec<u8>,
    },
    CallRespPartial {
        call_id: CallId,
        body : Vec<u8>,
    },
    CallRespFinal {
        call_id: CallId,
        body : Vec<u8>,
    },
    Check {
        call_id: CallId,
        timeout_ts : u64
    },
    CheckResp {
        call_id: CallId,
        exists : bool,
    },
    Cancel {
        call_id: u64
    }
}

type Map<K, V> = std::collections::BTreeMap<K, V>;
type Resp = tokio::sync::mpsc::Sender<Vec<u8>>;

pub struct RpcRouter<const N : usize> {
    shards : [Shard; N]
}

struct Shard {
    pending_calls : Map<CallId, CallInfo>
}

struct CallInfo {
    peer : NodeId,
    ts : SystemTime,
    rx : Resp
}