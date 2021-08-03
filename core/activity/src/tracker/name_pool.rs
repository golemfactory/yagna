use std::collections::HashSet;
use std::sync::Arc;

#[derive(Default)]
pub struct NamePool {
    names: HashSet<Arc<str>>,
}

impl NamePool {
    pub fn alloc(&mut self, name: &str) -> Arc<str> {
        if let Some(v) = self.names.get(name) {
            v.clone()
        } else {
            let v: Arc<str> = name.into();
            self.names.insert(v.clone());
            v
        }
    }
}
