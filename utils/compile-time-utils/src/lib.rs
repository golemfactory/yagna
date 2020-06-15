#[macro_export]
macro_rules! define_version_string {
    () => {
        #[macro_use]
        extern crate lazy_static;

        lazy_static! {
            static ref VERSION: String = {
                match option_env!("GITHUB_SHA") {
                    Some(sha) => format!("{}-{}", env!("CARGO_PKG_VERSION"), &sha[0..8]),
                    None => String::from(env!("CARGO_PKG_VERSION")),
                }
            };
        }
    };
}
