pub use git_version::git_version;

#[macro_export]
macro_rules! crate_version_commit {
    () => {
        Box::leak(
            [
                env!("CARGO_PKG_VERSION"),
                ya_compile_time_utils::git_version!(prefix = "-", fallback = ""),
            ]
            .join("")
            .into_boxed_str(),
        ) as &str
    };
}
