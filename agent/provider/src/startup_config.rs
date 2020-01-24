use structopt::StructOpt;

#[derive(StructOpt)]
pub struct StartupConfig {
    pub auth: String,
    #[structopt(long = "market-host", default_value = "127.0.0.1:5001")]
    pub market_address: String,
    #[structopt(long = "activity-host", default_value = "127.0.0.1:7465")]
    pub activity_address: String,
}
