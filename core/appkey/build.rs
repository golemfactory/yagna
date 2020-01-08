fn main() {
    #[cfg(target_env = "msvc")]
    {
        vcpkg::Config::new()
            .emit_includes(true)
            .find_package("sqlite3")
            .unwrap();
    }
}
