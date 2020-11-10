/*
    Helper to store active accounts in the driver by <NodeId.to_string(), NodeId>.

    To use Accounts on your driver:
    - Add type AccountsRc to the struct, for example:
        struct SomePaymentDriver {
            active_accounts: AccountsRc,
        }
    - Implement get_accounts:
        fn get_accounts(&self) -> AccountsRefMut {
            self.active_accounts.borrow_mut()
        }
    - Make sure your "DriverService" subscribes to identity events
        bus::subscribe_to_identity_events(driver).await;
    - The PaymentDriver trait will keep the list updated

*/

// External crates
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

// Workspace uses
use ya_core_model::identity::event::Event as IdentityEvent;

// Local uses
use crate::driver::NodeId;

// Public types
pub type AccountsRc = Rc<RefCell<Accounts>>;

pub struct Accounts {
    accounts: HashMap<String, NodeId>,
}

impl Accounts {
    pub fn new_rc() -> AccountsRc {
        Rc::new(RefCell::new(Self::new()))
    }

    pub fn handle_event(&mut self, msg: IdentityEvent) {
        log::debug!("handle_event: {:?}", &msg);
        match msg {
            IdentityEvent::AccountLocked { identity } => self.remove_account(identity),
            IdentityEvent::AccountUnlocked { identity } => self.add_account(identity),
        }
    }

    pub fn list_accounts(&self) -> Vec<String> {
        let list = self.accounts.keys().cloned().collect();
        log::debug!("list_accounts: {:?}", &list);
        list
    }

    pub fn get_node_id(&self, account: &str) -> Option<NodeId> {
        let node_id = self.accounts.get(account).cloned();
        log::debug!("get_node_id: {:?}", &node_id);
        node_id
    }

    fn new() -> Self {
        Self {
            accounts: HashMap::new(),
        }
    }

    fn add_account(&mut self, account: NodeId) {
        self.accounts.insert(account.to_string(), account);
        log::info!("Account: {:?} is unlocked", account.to_string());
    }

    fn remove_account(&mut self, account: NodeId) {
        self.accounts.remove(&account.to_string());
        log::info!("Account: {:?} is locked", account.to_string());
    }
}
