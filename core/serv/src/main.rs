use actix_web::{get, middleware, App, HttpServer, Responder};
use anyhow::{Context, Result};
use std::{convert::TryInto, fmt::Debug, path::PathBuf};
use structopt::*;

pub(crate) mod configuration;
use configuration::{CliCtx, Complete};

#[derive(StructOpt, Debug)]
#[structopt(global_setting = clap::AppSettings::ColoredHelp)]
#[structopt(about = clap::crate_description!())]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
struct CliArgs {
    /// Daemon data dir
    #[structopt(short, long = "datadir")]
    #[structopt(set = clap::ArgSettings::Global)]
    data_dir: Option<PathBuf>,

    /// Daemon address
    #[structopt(short, long)]
    #[structopt(default_value = "127.0.0.1")]
    address: String,

    /// Daemon port
    #[structopt(short, long)]
    #[structopt(default_value = "7465")]
    port: u16,

    /// Return results in JSON format
    #[structopt(long)]
    #[structopt(set = clap::ArgSettings::Global)]
    json: bool,

    /// Enter interactive mode
    #[structopt(short, long)]
    interactive: bool,

    #[structopt(subcommand)]
    command: Command,
}

#[derive(StructOpt, Debug)]
pub enum Command {
    /// Core service usage
    #[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
    Service(Service),

    /// Identity management
    #[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
    Id(ya_identity::IdentityCommand),

    #[structopt(name = "complete")]
    #[structopt(setting = structopt::clap::AppSettings::Hidden)]
    Complete(Complete),
}

impl Command {
    fn run_command(&self, ctx: &mut CliCtx) -> Result<()> {
        match self {
            Command::Service(service) => service.run_command(ctx),
            Command::Id(id) => id.run_command(),
            Command::Complete(complete) => complete.run_command(),
        }
    }
}

#[derive(StructOpt, Debug)]
pub enum Service {
    /// Runs server in foreground
    Run,
    /// Spawns daemon
    Start,
    /// Stops daemonm
    Stop,
    /// Checks if daemon is running
    Status,
}

impl Service {
    fn run_command(&self, ctx: &CliCtx) -> Result<()> {
        match self {
            Self::Run => {
                println!("Running {} service!", structopt::clap::crate_name!());
                Ok(HttpServer::new(|| {
                    App::new()
                        .wrap(middleware::Logger::default())
                        .service(index)
                })
                .bind(ctx.address())
                .context(format!("Failed to bind {:?}", ctx.address()))?
                .run()?)
            }
            _ => anyhow::bail!("command service {:?} is not implemented yet", self),
        }
    }
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

    pub fn get_address(&self) -> Result<(String, u16)> {
        Ok((self.address.clone(), self.port))
    }

    fn run_command(&self) -> Result<()> {
        let mut ctx: CliCtx = self.try_into()?;
        self.command.run_command(&mut ctx)
        //                ctx.output(resp?);
    }
}

#[get("/")]
fn index() -> impl Responder {
    format!("Hello {}!", clap::crate_description!())
}

fn main() -> Result<()> {
    let args = CliArgs::from_args();

    flexi_logger::Logger::with_env_or_str("actix_server=info,actix_web=info")
        .start()
        .unwrap();

    args.run_command()
}
