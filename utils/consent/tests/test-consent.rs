use std::env;
use ya_consent::api::set_consent;
use ya_consent::ConsentType;

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

    env::set_var("YA_CONSENT_PATH", format!("tmp-{}.txt", rand_string));
    env_logger::init();

    {
        set_consent(ConsentType::External, Some(false));
        let consent = ya_consent::api::have_consent(ConsentType::External);
        //remove file
        assert_eq!(consent, Some(false));
    }
    {
        set_consent(ConsentType::Internal, Some(true));

        let consent = ya_consent::api::have_consent(ConsentType::Internal);
        assert_eq!(consent, Some(true));
    }
    {
        set_consent(ConsentType::External, Some(true));

        let consent = ya_consent::api::have_consent(ConsentType::External);
        assert_eq!(consent, Some(true));
    }
    {
        set_consent(ConsentType::External, None);

        let consent = ya_consent::api::have_consent(ConsentType::External);
        assert_eq!(consent, None);
    }
}