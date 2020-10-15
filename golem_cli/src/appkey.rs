use crate::utils::{get_command_json_output, get_command_output, move_string_out_of_json};
use anyhow::Result;

struct AppKey {
    name: String,
    key: String,
}

fn appkey_from_json_as_in_list(mut value: serde_json::Value) -> Option<AppKey> {
    Some(AppKey {
        name: move_string_out_of_json(value.get_mut(0)?.take())?,
        key: move_string_out_of_json(value.get_mut(1)?.take())?,
    })
}

fn get_existing_key_from_output(mut command_output: serde_json::Value) -> Option<String> {
    let mut keys = command_output.get_mut("values")?.take();
    let keys = keys.as_array_mut()?;
    let mut keys = keys
        .drain(..)
        .filter_map(appkey_from_json_as_in_list)
        .collect::<Vec<_>>();

    let key = keys.drain(..).find(|appkey| appkey.name == "golem-cli")?;
    Some(key.key)
}

async fn get_existing_key() -> Result<Option<String>> {
    let keys = get_command_json_output("yagna", &["app-key", "list", "--json"]).await?;
    Ok(get_existing_key_from_output(keys))
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
