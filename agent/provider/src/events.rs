use crate::config::presets::Presets;
use crate::shutdown::ShutdownStatus;

#[derive(Clone, Debug)]
pub enum Event {
    Initialized,
    HardwareChanged,
    PresetsChanged {
        presets: Presets,
        updated: Vec<String>,
        removed: Vec<String>,
    },
    ShutdownChanged {
        status: ShutdownStatus,
    },
}
