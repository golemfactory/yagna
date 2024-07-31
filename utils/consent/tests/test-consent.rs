use std::env;
use ya_utils_consent::set_consent;
use ya_utils_consent::ConsentScope;

#[test]
pub fn test_save_and_load_entries() {
    use rand::Rng;
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "debug");
    }
    let rand_string: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(10)
        .map(char::from)
        .collect();

    let consent_path = format!("tmp-{}.txt", rand_string);
    env::set_var("YA_CONSENT_PATH", &consent_path);
    env_logger::init();

    {
        set_consent(ConsentScope::Internal, Some(true));

        let consent = ya_utils_consent::have_consent_cached(ConsentScope::Internal);
        assert_eq!(consent.consent, Some(true));
    }
    std::fs::remove_file(&consent_path).unwrap();
}
