use futures::prelude::*;
use iroh_net::discovery::dns::DnsDiscovery;
use iroh_net::discovery::local_swarm_discovery::LocalSwarmDiscovery;
use iroh_net::discovery::pkarr::dht::DhtDiscovery;
use iroh_net::discovery::pkarr::PkarrPublisher;
use iroh_net::discovery::ConcurrentDiscovery;
use iroh_net::key::SecretKey;
use iroh_net::Endpoint;

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let secret_key = SecretKey::generate();
    let id = secret_key.public();
    let dht = DhtDiscovery::builder()
        .dht(true)
        .secret_key(secret_key.clone())
        .build()?;

    let discovery = ConcurrentDiscovery::from_services(vec![
        Box::new(LocalSwarmDiscovery::new(id)?),
        Box::new(dht),
    ]);
    let ep = Endpoint::builder()
        .secret_key(secret_key)
        .discovery(Box::new(discovery))
        .bind()
        .await?;

    ep.discovery()
        .unwrap()
        .subscribe()
        .unwrap()
        .for_each(|di| {
            eprintln!("di: {:?}", di);
            async { () }
        })
        .await;

    let addr = ep.node_addr().await?;
    eprintln!("node addr = {:?}", addr);

    Ok(())
}
