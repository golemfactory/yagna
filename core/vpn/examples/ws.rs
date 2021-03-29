use actix::prelude::*;
use actix_web_actors::ws;
use actix_web_actors::ws::Frame;
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use std::path::PathBuf;
use structopt::StructOpt;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;
use ya_client::net::NetRequestorApi;
use ya_client::web::WebClient;
use ya_client_model::net::{Address, CreateNetwork, Network, Node};

#[derive(StructOpt, Clone, Debug)]
struct Cli {
    #[structopt(long)]
    api_url: Option<String>,
    #[structopt(long)]
    app_key: Option<String>,
    #[structopt(long)]
    net_id: Option<String>,
    #[structopt(long)]
    net_addr: Option<String>,
    #[structopt(long)]
    net_requestor_addr: Option<String>,
    #[structopt(long)]
    input_file: PathBuf,
    #[structopt(short, long)]
    output_file: PathBuf,
    #[structopt(short, long)]
    skip_create: bool,
    id: String,
    host: String,
    port: u16,
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::from_args();

    let api_url = match &cli.api_url {
        Some(_) => cli.api_url,
        None => std::env::var("YAGNA_API_URL").ok(),
    }
    .unwrap_or("http://127.0.0.1:7464".to_string());
    let app_key = match &cli.app_key {
        Some(app_key) => Some(app_key.clone()),
        None => std::env::var("YAGNA_APPKEY").ok(),
    }
    .ok_or_else(|| anyhow::anyhow!("Missing application key"))?;

    println!("Opening input file: {}", cli.input_file.display());
    let mut input = OpenOptions::new().read(true).open(cli.input_file).await?;

    println!("Opening output file: {}", cli.output_file.display());
    let mut output = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(cli.output_file)
        .await?;

    let net_id = cli
        .net_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_simple().to_string());

    let net_address;
    let net_requestor_address;

    if let Some(addr) = cli.net_addr {
        net_address = addr;
        net_requestor_address = cli
            .net_requestor_addr
            .ok_or_else(|| anyhow::anyhow!("Missing requestor address"))?;
    } else {
        net_address = "10.0.0.0".to_string();
        net_requestor_address = "10.0.0.1".to_string();
    }

    let client = WebClient::builder()
        .api_url(Url::parse(&api_url)?)
        .auth_token(&app_key)
        .build();
    let api: NetRequestorApi = client.interface()?;

    if cli.skip_create {
        println!("Re-using network: {}", net_id);
    } else {
        let msg = CreateNetwork {
            network: Network {
                id: net_id.clone(),
                ip: net_address,
                mask: None,
                gateway: None,
            },
        };

        println!("Creating network: {}", net_id);

        api.create_network(&msg).await?;
        api.add_address(
            &net_id,
            &Address {
                ip: net_requestor_address,
            },
        )
        .await?;
        api.add_node(
            &net_id,
            &Node {
                id: cli.id.clone(),
                ip: cli.host.clone(),
            },
        )
        .await?;
    }

    println!("Connecting to: {}:{}", cli.host, cli.port);

    let (response, connection) = api.connect_tcp(&net_id, &cli.host, cli.port).await?;
    let (mut sink, mut stream) = connection.split();

    println!("Response status: {:?}", response.status());

    Arbiter::spawn(async move {
        let mut buf = [0u8; 65535 - 14];
        loop {
            let read = input.read(&mut buf).await;
            let size = match read {
                Ok(0) => {
                    println!("EOF");
                    break;
                }
                Ok(s) => s,
                Err(e) => {
                    eprintln!("File read error: {}", e);
                    break Arbiter::current().stop();
                }
            };

            let bytes = Bytes::from(buf[..size].to_vec());
            if let Err(e) = sink.send(ws::Message::Binary(bytes)).await {
                eprintln!("Error sending data: {}", e);
                break Arbiter::current().stop();
            }
        }
    });

    while let Some(data) = stream.next().await {
        let frame = data.map_err(|e| anyhow::anyhow!("Protocol error: {}", e))?;
        let bytes = match frame {
            Frame::Text(bytes) => bytes,
            Frame::Binary(bytes) => bytes,
            Frame::Close(reason) => {
                println!("WebSocket connection closed: {:?}", reason);
                break;
            }
            Frame::Continuation(_) | Frame::Ping(_) | Frame::Pong(_) => continue,
        }
        .to_vec();
        output.write_all(&bytes).await?;
    }

    Ok(())
}
