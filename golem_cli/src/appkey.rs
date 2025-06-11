use crate::utils::{get_command_json_output, get_command_output, move_string_out_of_json};
use anyhow::Result;

#[allow(dead_code)]
#[derive(serde::Deserialize)]
struct AppKey {
    name: String,
    key: String,
    id: String,
    role: String,
    created: String,
}

fn appkey_from_json_as_in_list_compat(mut value: serde_json::Value) -> Option<AppKey> {
    Some(AppKey {
        name: move_string_out_of_json(value.get_mut(0)?.take())?,
        key: move_string_out_of_json(value.get_mut(1)?.take())?,
        id: move_string_out_of_json(value.get_mut(2)?.take())?,
        role: move_string_out_of_json(value.get_mut(3)?.take())?,
        created: move_string_out_of_json(value.get_mut(4)?.take())?,
    })
}

fn appkey_from_json_as_in_list(mut value: serde_json::Value) -> Option<AppKey> {
    Some(AppKey {
        name: move_string_out_of_json(value.get_mut("name")?.take())?,
        key: move_string_out_of_json(value.get_mut("key")?.take())?,
        id: move_string_out_of_json(value.get_mut("id")?.take())?,
        role: move_string_out_of_json(value.get_mut("role")?.take())?,
        created: move_string_out_of_json(value.get_mut("created")?.take())?,
    })
}

fn get_existing_key_from_output(mut command_output: serde_json::Value) -> Option<AppKey> {
    let mut keys = command_output
        .as_array_mut()?
        .drain(..)
        .filter_map(appkey_from_json_as_in_list)
        .collect::<Vec<_>>();

    let key = keys.drain(..).find(|appkey| appkey.name == "golem-cli")?;
    Some(key)
}

fn get_existing_key_from_output_compat(mut command_output: serde_json::Value) -> Option<AppKey> {
    let mut keys = command_output
        .as_array_mut()?
        .drain(..)
        .filter_map(appkey_from_json_as_in_list_compat)
        .collect::<Vec<_>>();

    let key = keys.drain(..).find(|appkey| appkey.name == "golem-cli")?;
    Some(key)
}

async fn get_existing_key() -> Result<Option<String>> {
    Ok(get_existing_app_key().await?.map(|appkey| appkey.key))
}

async fn get_existing_app_key() -> Result<Option<AppKey>> {
    let keys = get_command_json_output("yagna", &["app-key", "list", "--json"]).await?;
    log::info!(
        "get_existing_app_key keys: {:?}",
        serde_json::to_string(&keys)?
    );
    parse_appkey(keys)
}

pub async fn get_app_key() -> Result<String> {
    if let Some(key) = get_existing_key().await? {
        return Ok(key);
    }
    Ok(
        get_command_output("yagna", &["app-key", "create", "golem-cli"])
            .await?
            .trim_end()
            .to_string(),
    )
}

pub async fn get_identity_for_app_key() -> Result<Option<String>> {
    let appkey = get_existing_app_key().await?;
    Ok(appkey.map(|appkey| appkey.id))
}

pub(self) fn parse_appkey(mut keys: serde_json::Value) -> Result<Option<AppKey>> {
    Ok(match keys.get_mut("values") {
        Some(values) => get_existing_key_from_output_compat(values.take()),
        None => get_existing_key_from_output(keys),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_appkey() {
        let json_str = r#"[
            {
                "created": "2021-02-25T12:06:08.595731522",
                "id": "0x4f597d426bc06ed463cd2639cd5451667f9c3e3d",
                "key": "3c51522526e44ee690305224022108ca",
                "name": "requestor",
                "role": "manager"
            },
            {
                "created": "2021-05-20T18:09:32.689115524",
                "id": "0x4f597d426bc06ed463cd2639cd5451667f9c3e3d",
                "key": "3398660aa14c44e7b20b1787e6f06fe3",
                "name": "golem-cli",
                "role": "manager"
            },
            {
                "created": "2021-06-08T10:37:19.415101477",
                "id": "0xf98bb0842a7e744beedd291c98e7cd2c9b27f300",
                "key": "faa5649ef7f049598aba57bd1f66b5ee",
                "name": "testsession-requestor",
                "role": "manager"
            },
            {
                "created": "2023-09-21T13:32:42.410236642",
                "id": "0xf98bb0842a7e744beedd291c98e7cd2c9b27f300",
                "key": "d821538ec9bb4cf8b85a0a747e4789e2",
                "name": "ray-on-golem",
                "role": "manager"
            }
        ]"#;

        let json_value = serde_json::from_str(json_str).unwrap();
        let result = parse_appkey(json_value).unwrap();

        let appkey = result.unwrap();
        assert_eq!(appkey.name, "golem-cli");
        assert_eq!(appkey.key, "3398660aa14c44e7b20b1787e6f06fe3");
        assert_eq!(appkey.id, "0x4f597d426bc06ed463cd2639cd5451667f9c3e3d");
        assert_eq!(appkey.role, "manager");
        assert_eq!(appkey.created, "2021-05-20T18:09:32.689115524");
    }

    #[test]
    fn test_parse_appkey_without_golem_cli() {
        let json_str = r#"[
            {
                "created": "2021-02-25T12:06:08.595731522",
                "id": "0x4f597d426bc06ed463cd2639cd5451667f9c3e3d",
                "key": "3c51522526e44ee690305224022108ca",
                "name": "requestor",
                "role": "manager"
            }
        ]"#;

        let json_value = serde_json::from_str(json_str).unwrap();
        let result = parse_appkey(json_value).unwrap();

        assert!(
            result.is_none(),
            "Expected None when golem-cli entry is missing"
        );
    }
}
