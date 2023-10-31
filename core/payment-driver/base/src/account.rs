/*
    Helper to store active accounts in the driver by <NodeId.to_string(), NodeId>.
*/

// External crates
use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
// Workspace uses
use ya_core_model::identity::event::Event as IdentityEvent;

// Local uses
use crate::driver::NodeId;

// Public types
pub type AccountsArc = Arc<Mutex<Accounts>>;

pub struct Accounts {
    accounts: HashMap<String, NodeId>,
}

impl Accounts {
    pub fn new_rc() -> AccountsArc {
        Arc::new(Mutex::new(Self::new()))
    }

    pub fn handle_event(&mut self, msg: IdentityEvent) {
        log::trace!("handle_event: {:?}", &msg);
        match msg {
            IdentityEvent::AccountLocked { identity } => self.remove_account(identity),
            IdentityEvent::AccountUnlocked { identity } => self.add_account(identity),
        }
    }

    pub fn list_accounts(&self) -> Vec<String> {
        let list = self.accounts.keys().cloned().collect();
        log::trace!("list_accounts: {:?}", &list);
        list
    }

    pub fn get_node_id(&self, account: &str) -> Option<NodeId> {
        let node_id = self.accounts.get(account).cloned();
        log::trace!("get_node_id: {:?}", &node_id);
        node_id
    }

    fn new() -> Self {
        Self {
            accounts: HashMap::new(),
        }
    }

    pub fn add_account(&mut self, account: NodeId) {
        self.accounts.insert(account.to_string(), account);
        log::debug!("Account: {:?} is unlocked", account.to_string());
    }

    fn remove_account(&mut self, account: NodeId) {
        self.accounts.remove(&account.to_string());
        log::debug!("Account: {:?} is locked", account.to_string());
    }
}
