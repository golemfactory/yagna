use crate::config::presets::Presets;

#[derive(Clone, Debug)]
pub enum Event {
    Initialized,
    HardwareChanged,
    PresetsChanged {
        presets: Presets,
        updated: Vec<String>,
        removed: Vec<String>,
    },
}
