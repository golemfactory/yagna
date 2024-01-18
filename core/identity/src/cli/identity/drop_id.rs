use super::{bus, identity, NodeOrAlias, RpcEndpoint, CommandOutput, Result};

async fn prompt(message: &str, question: &str) -> anyhow::Result<bool> {
    use tokio::io::{self, AsyncWriteExt};

    let mut err = io::stderr();

    err.write_all(message.as_bytes()).await?;
    err.write_all("\n\n".as_bytes()).await?;
    err.flush().await?;

    let question = question.to_owned();
    let v = tokio::task::spawn_blocking(|| {
        let r : bool = promptly::prompt(question)?;

        Ok::<_, anyhow::Error>(r)
    }).await??;
    Ok(v)
}

pub async fn drop_id(node_or_alias: &NodeOrAlias, force: bool) -> Result<CommandOutput> {
    let command: identity::Get = node_or_alias.clone().into();
    let id = bus::service(identity::BUS_ID)
        .send(command)
        .await
        .map_err(anyhow::Error::msg)?;
    let id = match id {
        Ok(Some(v)) => v,
        Err(e) => return CommandOutput::object(Err::<(), _>(e)),
        Ok(None) => anyhow::bail!("Identity not found"),
    };
    if id.is_default {
        anyhow::bail!("Default identity cannot be dropped")
    }

    if id.deleted {
        anyhow::bail!("Identity is already deleted")
    }

    if !force {
        let confirm = if id.is_locked {
            prompt("By deleting this identity, you will also delete the internal key to your payment wallet.\n\
                   If you have any funds on it and haven't backed up the key, you won't be able to recover them. \n\
                   If you have any open contracts or pending payments on this identity, you won't be able to close or settle them.",
                   "Do you understand the consequences and still want to proceed [y/N]",
                   ).await?
        } else {
            prompt(
                "The identity is currently in use. After its deletion,\n\
                    a server restart will be necessary for the changes to take effect.\n\
                    By deleting this identity, you will also delete the internal key to your payment wallet.\n\
                    If you have any funds on it and haven't backed up the key, you won't be able to\n\
                    recover them. If you have any open contracts or pending payments on this identity,\n\
                    you won't be able to close or settle them.", 
                   "Do you understand the consequences and still want to proceed [y/N]"
            ).await?
        };

        if !confirm {
            return CommandOutput::none();
        }
    }

    CommandOutput::object(
        bus::service(identity::BUS_ID)
            .send(identity::DropId::with_id(id.node_id))
            .await
            .map_err(anyhow::Error::msg)?,
    )
}
