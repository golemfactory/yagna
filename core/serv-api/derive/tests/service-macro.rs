use actix_service::Service as ActixService;
use actix_web::{
    http::StatusCode,
    test::{init_service, TestRequest},
    web, HttpResponse,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use ya_service_api_derive::services;
use ya_service_api_interfaces::Provider;

pub struct CommandOutput;
pub struct CliCtx;

type Map = Rc<RefCell<HashMap<String, u8>>>;

#[derive(Default)]
pub struct ServiceContext {
    map: Map,
}

impl<Service> Provider<Service, Map> for ServiceContext {
    fn component(&self) -> Map {
        self.map.clone()
    }
}

fn inc<S, Context: Provider<S, Map>>(ctx: &Context, name: &str) {
    *ctx.component().borrow_mut().entry(name.into()).or_insert(0) += 1;
}

pub mod gsb_rest {
    pub use super::*;

    pub struct GsbRest;

    impl ya_service_api_interfaces::Service for GsbRest {
        type Cli = ();
    }

    impl GsbRest {
        pub async fn gsb<Context: Provider<Self, Map>>(ctx: &Context) -> anyhow::Result<()> {
            inc(ctx, "GsbRest-gsb");
            Ok(())
        }

        pub fn rest<Context: Provider<Self, Map>>(ctx: &Context) -> actix_web::Scope {
            inc(ctx, "GsbRest-rest");
            actix_web::Scope::new("/gsb-rest-api")
                .service(web::resource("/test").to(|| HttpResponse::Ok()))
        }
    }
}

pub mod gsb_cli {
    pub use super::*;
    use structopt::StructOpt;

    pub struct GsbCli;

    impl ya_service_api_interfaces::Service for GsbCli {
        type Cli = Commands;
    }

    impl GsbCli {
        pub async fn gsb<Context: Provider<Self, Map>>(ctx: &Context) -> anyhow::Result<()> {
            inc(ctx, "GsbCli-gsb");
            Ok(())
        }
    }

    #[derive(StructOpt, Debug)]
    /// gsb_cli command help
    pub enum Commands {
        /// foo subcommand help
        Foo(FooCommand),
        /// bar subcommand help
        Bar(BarCommand),
    }

    impl Commands {
        pub async fn run_command(self, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
            match self {
                Commands::Foo(command) => command.run_command(ctx).await,
                Commands::Bar(command) => command.run_command(ctx).await,
            }
        }
    }

    #[derive(StructOpt, Debug)]
    pub enum FooCommand {
        /// yes; bar help
        Yes,
    }

    impl FooCommand {
        pub async fn run_command(self, _: &CliCtx) -> anyhow::Result<CommandOutput> {
            Ok(CommandOutput {})
        }
    }

    #[derive(StructOpt, Debug)]
    pub enum BarCommand {
        /// no; bar help
        No,
    }

    impl BarCommand {
        pub async fn run_command(self, _: &CliCtx) -> anyhow::Result<CommandOutput> {
            Ok(CommandOutput {})
        }
    }
}

pub mod rest_cli {
    pub use super::*;
    use structopt::StructOpt;

    pub struct RestCli;

    impl ya_service_api_interfaces::Service for RestCli {
        type Cli = Commands;
    }

    #[derive(StructOpt, Debug)]
    /// rest_cli command help
    pub struct Commands {
        /// baz flag help
        #[structopt(short, long)]
        baz: bool,
    }

    impl Commands {
        pub async fn run_command(self, _ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
            Ok(CommandOutput {})
        }
    }

    impl RestCli {
        pub fn rest<Context: Provider<Self, Map>>(ctx: &Context) -> actix_web::Scope {
            inc(ctx, "RestCli-rest");
            actix_web::Scope::new("/rest-cli-api")
                .service(web::resource("/tester").to(|| HttpResponse::Ok()))
        }
    }
}

#[services(ServiceContext)]
#[derive(PartialEq)]
enum Services {
    #[enable(gsb, rest)]
    GsbRest(gsb_rest::GsbRest),
    #[enable(rest, cli)]
    RestCli(rest_cli::RestCli),
    #[enable(cli(flatten), gsb)]
    GsbCli(gsb_cli::GsbCli),
}

#[actix_rt::test]
async fn test_gsb() {
    // given
    let context = ServiceContext::default();
    assert_eq!(0, context.map.borrow().len());

    // when
    Services::gsb(&context).await.unwrap();

    // then
    assert_eq!(2, context.map.borrow().len());
    assert_eq!(&1, context.map.borrow().get("GsbRest-gsb").unwrap());
    assert_eq!(&1, context.map.borrow().get("GsbCli-gsb").unwrap());
}

#[actix_rt::test]
async fn test_rest() {
    // given
    let context = ServiceContext::default();
    assert_eq!(0, context.map.borrow().len());

    // when
    let app_with_srv = Services::rest(actix_web::App::new(), &context);
    let srv = init_service(app_with_srv).await;

    // then
    assert_eq!(2, context.map.borrow().len());
    assert_eq!(&1, context.map.borrow().get("GsbRest-rest").unwrap());
    assert_eq!(&1, context.map.borrow().get("RestCli-rest").unwrap());

    let req = TestRequest::with_uri("/gsb-rest-api/test").to_request();
    let resp = srv.call(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let req = TestRequest::with_uri("/rest-cli-api/tester").to_request();
    let resp = srv.call(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let req = TestRequest::with_uri("/blah").to_request();
    let resp = srv.call(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[test]
fn test_cli_help() {
    // given
    use structopt::StructOpt;
    let mut out = Vec::new();
    Services::clap().write_long_help(&mut out).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out),
        "ya-service-api-derive 0.1.0
gsb_cli command help

USAGE:
    ya-service-api-derive <SUBCOMMAND>

FLAGS:
    -h, --help       
            Prints help information

    -V, --version    
            Prints version information


SUBCOMMANDS:
    rest-cli    rest_cli command help
    foo         foo subcommand help
    bar         bar subcommand help
    help        Prints this message or the help of the given subcommand(s)"
    )
}

#[test]
fn test_cli() {
    use structopt::StructOpt;
    match Services::from_iter(&["app name", "foo", "yes"]) {
        Services::GsbCli(gsb_cli::Commands::Foo(gsb_cli::FooCommand::Yes)) => (),
        _ => panic!(),
    }

    match Services::from_iter(&["app name", "bar", "no"]) {
        Services::GsbCli(gsb_cli::Commands::Bar(gsb_cli::BarCommand::No)) => (),
        _ => panic!(),
    }
}
