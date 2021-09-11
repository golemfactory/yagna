#[derive(Clone, Debug, StructOpt)]
struct Args {
    platform: String,
    order_id: String,
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {}
