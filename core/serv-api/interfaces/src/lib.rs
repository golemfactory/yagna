pub trait Service {
    type Cli;
}

pub trait Provider<Service, Component> {
    fn component(&self) -> Component;
}

impl<Service, Component: Clone> Provider<Service, Component> for Component {
    fn component(&self) -> Component {
        self.clone()
    }
}
