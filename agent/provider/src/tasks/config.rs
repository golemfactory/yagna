use humantime;
use structopt::StructOpt;

/// Configuration for TaskManager actor.
#[derive(StructOpt, Clone, Debug)]
pub struct TaskConfig {
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "90s")]
    pub idle_agreement_timeout: std::time::Duration,
}
