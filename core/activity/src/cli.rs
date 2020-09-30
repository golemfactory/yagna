/// Payment management.
#[derive(StructOpt, Debug)]
pub enum PaymentCli {
    Status {
        #[structopt(long)]
        id: Option<String>,
    },
}
