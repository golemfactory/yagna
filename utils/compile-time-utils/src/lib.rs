pub use git_version::git_version;

#[macro_export]
macro_rules! crate_version_commit {
    () => {
        Box::leak(
            [
                ya_compile_time_utils::git_version!(
                    args = ["--tag"],
                    fallback = env!("CARGO_PKG_VERSION")
                ),
                &option_env!("GITHUB_RUN_NUMBER")
                    .map(|run_no| format!("-b{}", run_no))
                    .unwrap_or("".to_string()),
            ]
            .join("")
            .into_boxed_str(),
        ) as &str
    };
}
