use anyhow::Context;
use futures::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use ya_core_model::identity;
use ya_service_api::constants::CENTRAL_NET_HOST;
use ya_service_api_interfaces::Service;
use ya_service_bus::typed as bus;
use ya_service_bus::RpcEndpoint;

pub struct Net<Db>(PhantomData<Db>);

impl<Db> Service for Net<Db> {
    type Db = Db;
    type Cli = ();

    fn gsb<'f>(_: &'f Self::Db) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + 'f>> {
        Box::pin(async move {
            let default_id = bus::service(identity::BUS_ID)
                .send(identity::Get::ByDefault)
                .await
                .map_err(anyhow::Error::msg)??
                .ok_or(anyhow::Error::msg("no default identity"))?
                .node_id
                .to_string();
            log::info!("using default identity as network id: {:?}", default_id);
            crate::bind_remote(&*CENTRAL_NET_HOST, &default_id)
                .await
                .context(format!(
                    "Error binding network service at {} for {}",
                    *CENTRAL_NET_HOST, &default_id
                ))?;

            Ok(())
        })
    }
}
