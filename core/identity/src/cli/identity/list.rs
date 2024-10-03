use super::*;
use ya_core_model::bus::GsbBindPoints;
pub async fn list(gsb: &GsbBindPoints) -> Result<CommandOutput> {
    let mut identities: Vec<identity::IdentityInfo> = gsb
        .local()
        .send(identity::List::default())
        .await
        .map_err(anyhow::Error::msg)
        .context("sending id List to BUS")?
        .unwrap();
    identities.sort_by_key(|id| Reverse((id.is_default, id.alias.clone())));
    Ok(ResponseTable {
        columns: vec![
            "default".into(),
            "locked".into(),
            "delete in progress".into(),
            "alias".into(),
            "address".into(),
        ],
        values: identities
            .into_iter()
            .map(|identity| {
                serde_json::json! {[
                    if identity.is_default { "X" } else { "" },
                    if identity.is_locked { "X" } else { "" },
                    if identity.deleted { "X" } else { "" },
                    identity.alias,
                    identity.node_id
                ]}
            })
            .collect(),
    }
    .into())
}
