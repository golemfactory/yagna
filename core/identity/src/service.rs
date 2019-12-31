/// Identity service

use std::error::Error;
use ya_core_model::identity as model;
use ya_service_bus::typed as bus;

// TODO: is anyhow appropriate Error here?
pub async fn activate() -> Result<(), anyhow::Error> {
    // TODO: move real logic here
    let _ = bus::bind(model::ID, |_: model::List| {
        eprintln!("ask for");
        async {
            Ok(vec![model::IdentityInfo {
                alias: "mock".to_string(),
                node_id: "mock".to_string(),
                is_locked: false,
            }])
        }
    });
    let _ = bus::bind(model::ID, |create: model::CreateGenerated| {
        eprintln!("create generated called: {:?}", create);
        async {
            Ok(model::IdentityInfo {
                alias: create.alias.unwrap_or_else(|| "default".into()),
                node_id: "fake node_id".to_string(),
                is_locked: false,
            })
        }
    });
    eprintln!("started");

    Ok(())
}
