use chrono::Utc;

use ya_identity::dao::{identity::*, init};
use ya_persistence::executor::DbExecutor;

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();
    let db = DbExecutor::from_env()?;

    init(&db).await?;

    let identity = Identity {
        identity_id: "0x1308f7345c455ED528bC80C37C7EC175Abe502B5".parse()?,
        key_file_json: "".to_string(),
        is_default: false,
        is_deleted: false,
        alias: None,
        note: None,
        created_date: Utc::now().naive_utc(),
    };

    db.as_dao::<IdentityDao>().create_identity(identity).await?;

    eprintln!(
        "v={:?}",
        db.as_dao::<IdentityDao>().list_identities().await?
    );

    Ok(())
}
