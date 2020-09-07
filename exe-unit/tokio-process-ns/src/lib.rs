#![cfg(target_os = "linux")]

mod pre_exec;

#[derive(Default, Clone, Copy)]
pub struct NsOptions {
    fork: bool,
    kill_child: bool,
    procfs: bool,
}

impl NsOptions {
    pub fn new() -> Self {
        NsOptions::default()
    }

    pub fn kill_child(mut self) -> Self {
        self.fork = true;
        self.kill_child = true;
        self
    }

    pub fn procfs(mut self) -> Self {
        self.procfs = true;
        self
    }
}

pub trait NsCommand {
    fn new_ns(&mut self, options: NsOptions) -> &mut Self;
}

impl NsCommand for tokio::process::Command {
    fn new_ns(&mut self, options: NsOptions) -> &mut Self {
        unsafe { self.pre_exec(move || pre_exec::pre_exec(options)) }
    }
}

impl NsCommand for std::process::Command {
    fn new_ns(&mut self, options: NsOptions) -> &mut Self {
        use std::os::unix::process::CommandExt;
        unsafe { self.pre_exec(move || pre_exec::pre_exec(options)) }
    }
}
