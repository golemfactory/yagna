use std::env;
use ya_consent::api::set_consent;
use ya_consent::ConsentType;

#[test]
pub fn test_save_and_load_entries() {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "debug");
    }
    env_logger::init();
    set_consent(ConsentType::External, false);

    let consent = ya_consent::api::have_consent(ConsentType::External).unwrap();
    assert!(!consent);


}