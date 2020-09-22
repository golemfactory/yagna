pub trait Service {
    type Cli;
}

pub trait Provider<Service, Component> {
    fn component(&self) -> Component;
}

impl<Service: 'static, Component> Provider<Service, ()> for Component {
    fn component(&self) -> () {}
}
