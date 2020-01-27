use super::dispatcher::Dispatcher;


pub struct FileDispatcher {

}


impl FileDispatcher {
    pub fn new() -> Box<dyn Dispatcher> {
        Box::new(FileDispatcher{})
    }
}

impl Dispatcher for FileDispatcher {

}

