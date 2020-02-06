use ya_model::activity::ExeScriptCommand;

use serde::de::DeserializeOwned;
use anyhow::{Result, Error};


/// Deserialize strign with json into ExeScriptCommands.
pub fn commands_from_json(json: &str) -> Result<Vec<ExeScriptCommand>> {
    match serde_json::from_str(json) {
        // OK, we got a JSON array, now interpret Values as strongly typed Cmd
        // in case we cannot interpret any command, throw an error but do not
        // interrupt the flow
        Ok(serde_json::Value::Array(cmds)) => {
            let result_vec = cmds.into_iter()
                .map(|cmd|{
                    serde_json::from_value::<ExeScriptCommand>(cmd)
                        .map_err(|error|{
                            Error::msg(format!("{}", error))
                        })
                })
                .collect::<Vec<Result<ExeScriptCommand>>>();

            result_vec.into_iter().collect::<Result<Vec<ExeScriptCommand>>>()
        },
        Ok(value) => Err(Error::msg(format!("Wrong json"))),
        Err(e) => Err(Error::msg(format!("Wrong json")))
    }
}

