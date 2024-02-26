use test_context::AsyncTestContext;

#[async_trait::async_trait]
pub trait AsyncDroppable: Send + Sync + 'static {
    async fn async_drop(&self);
}

pub struct DroppableTestContext {
    drops: Vec<Box<dyn AsyncDroppable>>,
}

impl DroppableTestContext {
    pub fn register(&mut self, droppable: impl AsyncDroppable) {
        self.drops.push(Box::new(droppable));
    }
}

#[async_trait::async_trait]
impl AsyncTestContext for DroppableTestContext {
    async fn setup() -> DroppableTestContext {
        DroppableTestContext { drops: vec![] }
    }

    async fn teardown(self) {
        for droppable in self.drops {
            droppable.async_drop().await;
        }
    }
}
