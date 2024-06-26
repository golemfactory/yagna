use std::env;
use std::path::Path;
use ya_consent::consent::{ConsentEntry, ConsentType, load_entries, save_entries};

#[test]
pub fn test_save_and_load_entries() {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "debug");
    }
    env_logger::init();
    let path = Path::new("test_consent.txt");
    let entries = vec![
        ConsentEntry {
            consent_type: ConsentType::External,
            allowed: false,
        },
        ConsentEntry {
            consent_type: ConsentType::Internal,
            allowed: true,
        }
    ];

    save_entries(path, entries.clone()).unwrap();
    let loaded_entries = load_entries(path);

    assert_eq!(entries, loaded_entries);
}