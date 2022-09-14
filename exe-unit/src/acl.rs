use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::rc::Rc;

use ya_client_model::NodeId;

pub type Acl = AccessControl<NodeId>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum AccessRole {
    Control,
    Host,
    Observe,
}

#[derive(thiserror::Error, Clone, Debug)]
pub enum Error {
    #[error("Forbidden call from {0}: role '{1:?}' not granted")]
    Forbidden(String, AccessRole),
}

#[derive(Clone, Default)]
pub struct AccessControl<K: Hash + Eq> {
    inner: Rc<RefCell<HashMap<K, HashSet<AccessRole>>>>,
}

impl<K: Hash + Eq + ToOwned<Owned = K>> AccessControl<K> {
    pub fn grant(&self, id: K, role: AccessRole) {
        self.inner
            .borrow_mut()
            .entry(id)
            .or_insert_with(Default::default)
            .insert(role);
    }
}

impl<K: Hash + Eq> AccessControl<K> {
    pub fn has_access<T: AsRef<K>>(&self, id: T, role: AccessRole) -> bool {
        self.inner
            .borrow()
            .get(id.as_ref())
            .map(|e| e.contains(&role))
            .unwrap_or(false)
    }

    pub fn revoke<T: AsRef<K>>(&self, id: T, role: AccessRole) -> bool {
        self.inner
            .borrow_mut()
            .get_mut(id.as_ref())
            .map(|e| e.remove(&role))
            .unwrap_or(false)
    }
}

impl<K: Hash + Eq + Clone> AccessControl<K> {
    pub fn controllers(&self) -> HashSet<K> {
        self.inner
            .borrow()
            .iter()
            .filter_map(|(k, r)| {
                if r.contains(&AccessRole::Control) {
                    Some(k.clone())
                } else {
                    None
                }
            })
            .collect()
    }
}

impl<K: Hash + Eq + std::fmt::Debug> std::fmt::Debug for AccessControl<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.borrow();
        write!(f, "{:?}", *inner)
    }
}
