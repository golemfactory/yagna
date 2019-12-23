use actix_web::{get, middleware, App, HttpServer, Responder};
use std::fmt::Debug;
use std::path::PathBuf;
use structopt::*;

#[derive(StructOpt, Debug)]
#[structopt(global_setting = clap::AppSettings::ColoredHelp)]
#[structopt(about = clap::crate_description!())]
struct CliArgs {
    #[cfg(feature = "interactive_cli")]
    /// Enter interactive mode
    #[structopt(short, long)]
    interactive: bool,

    /// Daemon address
    #[structopt(short, long)]
    #[structopt(display_order = 500)]
    #[structopt(set = clap::ArgSettings::Global)]
    address: Option<String>,

    /// Daemon port
    #[structopt(short, long)]
    #[structopt(display_order = 500)]
    #[structopt(set = clap::ArgSettings::Global)]
    port: Option<u16>,

    /// Daemon data dir
    #[structopt(short, long = "datadir")]
    #[structopt(set = clap::ArgSettings::Global)]
    data_dir: Option<PathBuf>,

    /// Return results in JSON format
    #[structopt(long)]
    #[structopt(display_order = 500)]
    #[structopt(set = clap::ArgSettings::Global)]
    json: bool,

    #[structopt(subcommand)]
    command: Option<Commands>,
}
#[derive(StructOpt, Debug)]
pub enum Commands {
    /// Identity Management
    #[structopt(name = "id")]
    #[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
    Id(ya_identity::IdentityCommand),
}

impl CliArgs {
    #[allow(dead_code)]
    pub fn get_data_dir(&self) -> PathBuf {
        match &self.data_dir {
            Some(data_dir) => data_dir.to_owned(),
            None => appdirs::user_data_dir(Some("yagna"), Some("golem"), false)
                .unwrap()
                .join("default"),
        }
    }

    pub fn get_address(&self) -> failure::Fallible<(&str, u16)> {
        let address = match &self.address {
            Some(a) => a.as_str(),
            None => "127.0.0.1",
        };

        Ok((address.into(), self.port.unwrap_or(7465)))
    }
}

#[get("/")]
fn index() -> impl Responder {
    format!("Hello {}!", clap::crate_description!())
}

fn main() -> failure::Fallible<()> {
    let args = CliArgs::from_args();

    flexi_logger::Logger::with_env_or_str("actix_server=info,actix_web=info")
        .start()
        .unwrap();

    println!("Hello {}!", clap::crate_description!());

    Ok(HttpServer::new(|| {
        App::new()
            .wrap(middleware::Logger::default())
            .service(index)
    })
    .bind(args.get_address()?)?
    .run()?)
}
