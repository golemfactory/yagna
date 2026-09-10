use thiserror::Error;

macro_rules! define_string_error {
    ($name:ident) => {
        #[derive(Debug, Clone, Error, PartialEq, Eq)]
        #[error("{msg}")]
        pub struct $name {
            pub msg: String,
        }

        impl $name {
            pub fn new(message: impl Into<String>) -> Self {
                Self {
                    msg: message.into(),
                }
            }
        }
    };
}

define_string_error!(ParseError);
define_string_error!(ResolveError);
define_string_error!(ExpressionError);
define_string_error!(PrepareError);
define_string_error!(MatchError);
