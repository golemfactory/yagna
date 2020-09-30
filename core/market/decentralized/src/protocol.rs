/// This can't be constant, because rust doesn't allow to concat! 'static &str
/// even if they are const variable.
#[macro_export]
macro_rules! PROTOCOL_VERSION {
    () => {
        "mk1"
    };
}

pub mod callback;
pub mod discovery;
pub mod negotiation;
