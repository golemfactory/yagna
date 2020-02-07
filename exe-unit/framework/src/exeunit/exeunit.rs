

/// Implement this trait to use ExeUnit framework.
pub trait ExeUnit: std::marker::Send {

    fn on_start(&mut self);
    fn on_deploy(&mut self);
    fn on_run(&mut self);
    fn on_transferred(&mut self);
    fn on_stop(&mut self);
}





