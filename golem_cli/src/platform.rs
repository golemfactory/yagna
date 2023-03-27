use std::borrow::Cow;
use std::ops::Not;

#[allow(dead_code)]
#[derive(PartialEq, Eq)]
pub enum Status {
    Valid,
    Permission(Cow<'static, str>),
    InvalidEnv(Cow<'static, str>),
    NotImplemented,
}

impl Status {
    pub fn is_implemented(&self) -> bool {
        matches!(self, Status::NotImplemented).not()
    }

    pub fn problem(&self) -> Option<&str> {
        match self {
            Status::Permission(msg) => Some(msg.as_ref()),
            Status::InvalidEnv(msg) => Some(msg.as_ref()),
            _ => None,
        }
    }

    pub fn is_valid(&self) -> bool {
        matches!(self, Status::Valid)
    }
}

#[cfg(target_os = "linux")]
pub fn kvm_status() -> Status {
    use nix::unistd::AccessFlags;
    use std::path;
    let dev_kvm = path::Path::new("/dev/kvm");
    if !dev_kvm.exists() {
        if path::Path::new("/dev/xen").exists() {
            return Status::InvalidEnv(Cow::Borrowed("unsupported virtualization type: XEN"));
        }
        if path::Path::new("/.dockerenv").exists() {
            return Status::InvalidEnv(Cow::Borrowed(
                "running inside Docker without access to /dev/kvm. For additional help see: https://handbook.golem.network/troubleshooting/provider-troubleshooting#invalid-vm",
            ));
        }
        return Status::Permission(Cow::Borrowed("kvm kernel module is not installed"));
    }
    match nix::unistd::access(dev_kvm, AccessFlags::W_OK | AccessFlags::R_OK) {
        Ok(()) => Status::Valid,
        Err(_) => Status::Permission(Cow::Borrowed("the user has no access to /dev/kvm. For additional help see: https://handbook.golem.network/troubleshooting/provider-troubleshooting#invalid-vm")),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn kvm_status() -> Status {
    Status::NotImplemented
}
