/// Macro that generates a `from_env()` function for a struct that derives `Parser`.
/// This function parses configuration from environment variables and command line arguments,
/// with environment variables taking precedence over command line arguments.
///
/// Usage: `define_from_env!(StructName)`
#[macro_export]
macro_rules! define_from_env {
    ($struct_name:ident) => {
        impl $struct_name {
            pub fn from_env() -> Result<$struct_name, clap::Error> {
                // Empty command line arguments, because we want to use ENV fallback
                // or default values if ENV variables are not set.
                $struct_name::try_parse_from([""])
            }
        }
    };
}
