/// Broadcast support service
// Note: This file is derived from core/net module. It serves only as mock
// so we don't have to keep it compatible.
// It was moved here, because this file is not expected to be public in net module.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use ya_core_model::net::local as local_net;

#[derive(Clone, Default)]
pub struct BCastService {
    inner: Rc<RefCell<BCastServiceInner>>,
}

#[derive(Default)]
struct BCastServiceInner {
    topics: BTreeMap<String, Vec<Rc<str>>>,
}

impl BCastService {
    pub fn add(&self, subscribe: local_net::Subscribe) {
        let mut me = self.inner.borrow_mut();
        me.topics
            .entry(subscribe.topic().to_owned())
            .or_insert_with(Default::default)
            .push(subscribe.endpoint().into())
    }

    pub fn resolve(&self, topic: &str) -> Vec<Rc<str>> {
        let me = self.inner.borrow();
        me.topics
            .get(topic)
            .map(|receivers| receivers.iter().map(|endpoint| endpoint.clone()).collect())
            .unwrap_or_default()
    }
}
