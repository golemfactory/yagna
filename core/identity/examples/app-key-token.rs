use serde_json::json;
use sha2::Digest;
use structopt::StructOpt;
use ya_core_model::{appkey, identity};
use ya_service_bus::typed as bus;
use ya_service_bus::RpcEndpoint;

// Tool for generating JWT tokens signed with identity key.
#[derive(StructOpt)]
enum Args {
    List,
    Gen {
        from_key: String,
    },
    SendKeys {
        #[structopt(default_value = "http://127.0.0.1:5001/admin/import-key")]
        to_url: String,
    },
}

fn jwt_encoded(v: serde_json::Value) -> anyhow::Result<String> {
    let mut wrapped_writer = Vec::new();
    {
        let mut enc =
            base64::write::EncoderWriter::new(&mut wrapped_writer, base64::URL_SAFE_NO_PAD);
        serde_json::to_writer(&mut enc, &v)?;
        enc.finish()?
    }
    Ok(String::from_utf8(wrapped_writer)?)
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let args = Args::from_args();
    match args {
        Args::Gen { from_key } => {
            let key = bus::service(appkey::BUS_ID)
                .send(appkey::Get::with_key(from_key))
                .await??;

            let now = std::time::SystemTime::now();
            let iat = now.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
            let node_id = key.identity;

            let header = json! {{
                "alg": "ES256",
                "typ": "JWT"
            }};
            let body = json! {{
                "sub": key.identity,
                "aud": key.key,
                "name": key.name,
                "role": key.role,
                "iat": iat
            }};
            let jwt_body = format!("{}.{}", jwt_encoded(header)?, jwt_encoded(body)?);
            let msg_hash = sha2::Sha256::digest(jwt_body.as_bytes()).to_vec();

            let signature = bus::service(identity::BUS_ID)
                .send(identity::Sign {
                    node_id,
                    payload: msg_hash,
                })
                .await??;
            eprintln!(
                "token= {}.{}",
                jwt_body,
                base64::encode_config(&signature, base64::URL_SAFE_NO_PAD)
            )
        }
        Args::SendKeys { to_url } => {
            let (ids, _n) = bus::service(appkey::BUS_ID)
                .send(appkey::List {
                    identity: None,
                    page: 1,
                    per_page: 10,
                })
                .await??;
            let ids: Vec<serde_json::Value> = ids
                .into_iter()
                .map(|k: appkey::AppKey| json! {{"key": k.key, "nodeId": k.identity}})
                .collect();
            serde_json::to_writer_pretty(&mut std::io::stderr(), &ids)?;
            let c = awc::Client::new();
            let resp: serde_json::Value = c
                .post(to_url)
                .send_json(&ids)
                .await
                .map_err(|e| anyhow::Error::msg(e.to_string()))?
                .json()
                .await
                .map_err(|e| anyhow::Error::msg(e.to_string()))?;
            eprintln!();
            eprintln!("response={}", serde_json::to_string_pretty(&resp)?);
        }
        Args::List => {
            let ids = bus::service(appkey::BUS_ID)
                .send(appkey::List {
                    identity: None,
                    page: 1,
                    per_page: 10,
                })
                .await?;
            eprintln!("{:?}", ids);
        }
    }
    eprintln!("ok");
    Ok(())
}
