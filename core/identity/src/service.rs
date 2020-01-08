/// Identity service
use std::error::Error;
use ya_core_model::identity as model;
use ya_service_bus::typed as bus;

// TODO: is anyhow appropriate Error here?
pub fn activate() -> Result<(), anyhow::Error> {
    // TODO: move real logic here
    log::info!("activating identity service");
    let _ = bus::bind(model::BUS_ID, |_: model::List| {
        log::debug!("asked for identity List");
        async {
            Ok(vec![model::IdentityInfo {
                alias: "mock".to_string(),
                node_id: "mock".to_string(),
                is_locked: false,
            }])
        }
    });
    let _ = bus::bind(model::BUS_ID, |create: model::CreateGenerated| {
        log::debug!("creating generated identity List: {:?}", create);
        async {
            Ok(model::IdentityInfo {
                alias: create.alias.unwrap_or_else(|| "default".into()),
                node_id: "fake node_id".to_string(),
                is_locked: false,
            })
        }
    });
    log::info!("identity service activated");

    Ok(())
}
