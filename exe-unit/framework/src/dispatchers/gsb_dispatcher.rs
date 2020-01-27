use super::dispatcher::Dispatcher;




pub struct GsbDispatcher {

}

impl GsbDispatcher {
    pub fn new() -> Box<dyn Dispatcher> {
        Box::new(GsbDispatcher{})
    }
}



impl Dispatcher for GsbDispatcher {

}

