use super::dispatcher::Dispatcher;


pub struct InteractiveCli {

}

impl InteractiveCli {
    pub fn new() -> Box<dyn Dispatcher> {
        Box::new(InteractiveCli{})
    }
}


impl Dispatcher for InteractiveCli {

}




