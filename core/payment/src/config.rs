use structopt::*;

#[derive(StructOpt, Clone)]
pub struct Config {
    #[structopt(flatten)]
    pub sync_notif_backoff: SyncNotifBackoffConfig,
}

#[derive(StructOpt, Clone)]
pub struct SyncNotifBackoffConfig {
    /// Enables background job for synchronizing invoice / debit note document status.
    ///
    /// This depends on the identity service being enabled to work. If you're working with a limited
    /// subsets of services (e.g. in payment_api.rs example) you might wish to disable that.
    #[structopt(parse(try_from_str), default_value = "true")]
    pub run_sync_job: bool,

    #[structopt(long, env = "YA_PAYMENT_SYNC_NOTIF_BACKOFF_INITIAL_DELAY", parse(try_from_str = humantime::parse_duration), default_value = "30s")]
    pub initial_delay: std::time::Duration,

    #[structopt(
        long,
        env = "YA_PAYMENT_SYNC_NOTIF_BACKOFF_EXPONENT",
        default_value = "6"
    )]
    pub exponent: f64,

    #[structopt(
        long,
        env = "YA_PAYMENT_SYNC_NOTIF_BACKOFF_MAX_RETRIES",
        default_value = "7"
    )]
    pub max_retries: u32,

    #[structopt(long, env = "YA_PAYMENT_SYNC_NOTIF_BACKOFF_CAP", parse(try_from_str = humantime::parse_duration))]
    pub cap: Option<std::time::Duration>,

    #[structopt(long, env = "YA_PAYMENT_SYNC_NOTIF_BACKOFF_ERROR_DELAY", parse(try_from_str = humantime::parse_duration), default_value = "10m")]
    pub error_delay: std::time::Duration,
}

impl Config {
    pub fn from_env() -> Result<Config, structopt::clap::Error> {
        // Empty command line arguments, because we want to use ENV fallback
        // or default values if ENV variables are not set.
        Config::from_iter_safe(&[""])
    }

    pub fn run_sync_job(mut self, value: bool) -> Self {
        self.sync_notif_backoff.run_sync_job = value;
        self
    }
}
