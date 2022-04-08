use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use ethsign::{PublicKey, Signature};
use futures::future::LocalBoxFuture;
use futures::FutureExt;

use ya_core_model::{identity, NodeId};
use ya_relay_client::crypto::{Crypto, CryptoProvider};
use ya_service_bus::RpcEndpoint;

pub struct IdentityCryptoProvider {
    default_id: NodeId,
    aliases: Rc<RefCell<AliasCache>>,
    cache: Rc<RefCell<HashMap<NodeId, Rc<dyn Crypto>>>>,
}

impl IdentityCryptoProvider {
    pub fn new(default_id: NodeId) -> Self {
        Self {
            default_id,
            aliases: Default::default(),
            cache: Default::default(),
        }
    }
}

impl CryptoProvider for IdentityCryptoProvider {
    fn default_id<'a>(&self) -> LocalBoxFuture<'a, anyhow::Result<NodeId>> {
        futures::future::ok(self.default_id).boxed_local()
    }

    fn aliases<'a>(&self) -> LocalBoxFuture<'a, anyhow::Result<Vec<NodeId>>> {
        let aliases = self.aliases.borrow();
        if aliases.valid_ttl() {
            return futures::future::ok(aliases.data.clone()).boxed_local();
        }

        let aliases_rfc = self.aliases.clone();
        async move {
            let identities = ya_service_bus::typed::service(identity::BUS_ID)
                .send(identity::List {})
                .await
                .map_err(anyhow::Error::msg)??;

            let node_ids: Vec<_> = identities
                .into_iter()
                .filter(|info| !(info.is_default || info.is_locked))
                .map(|info| info.node_id)
                .collect();

            let mut aliases = aliases_rfc.borrow_mut();
            aliases.update(node_ids.clone());

            Ok(node_ids)
        }
        .boxed_local()
    }

    fn get<'a>(&self, node_id: NodeId) -> LocalBoxFuture<'a, anyhow::Result<Rc<dyn Crypto>>> {
        if let Some(crypto) = (*self.cache.borrow()).get(&node_id).cloned() {
            return futures::future::ok(crypto).boxed_local();
        }

        let cache = self.cache.clone();
        async move {
            let bytes = ya_service_bus::typed::service(identity::BUS_ID)
                .send(identity::GetPubKey(node_id))
                .await
                .map_err(anyhow::Error::msg)??;

            let key =
                PublicKey::from_slice(&bytes).map_err(|_| anyhow::anyhow!("invalid public key"))?;
            let crypto: Box<dyn Crypto> = Box::new(IdentityCrypto::new(node_id, key));
            let crypto: Rc<dyn Crypto> = crypto.into();
            cache.borrow_mut().insert(node_id, crypto.clone());

            Ok(crypto)
        }
        .boxed_local()
    }
}

pub struct IdentityCrypto {
    node_id: NodeId,
    key: PublicKey,
    #[allow(unused)]
    created: Instant,
}

impl IdentityCrypto {
    pub fn new(node_id: NodeId, key: PublicKey) -> Self {
        Self {
            node_id,
            key,
            created: Instant::now(),
        }
    }
}

impl Crypto for IdentityCrypto {
    fn public_key<'a>(&self) -> LocalBoxFuture<'a, anyhow::Result<PublicKey>> {
        futures::future::ok(self.key.clone()).boxed_local()
    }

    fn sign<'a>(&self, message: &'a [u8]) -> LocalBoxFuture<'a, anyhow::Result<Signature>> {
        let node_id = self.node_id;
        let payload = message.to_vec();

        async move {
            let bytes = ya_service_bus::typed::service(identity::BUS_ID)
                .send(identity::Sign { node_id, payload })
                .await
                .map_err(anyhow::Error::msg)??;

            let v = bytes[0];
            let mut r = [0u8; 32];
            let mut s = [0u8; 32];
            r.copy_from_slice(&bytes[1..33]);
            s.copy_from_slice(&bytes[33..65]);

            Ok(Signature { v, r, s })
        }
        .boxed_local()
    }

    fn encrypt<'a>(
        &self,
        _message: &'a [u8],
        _remote_key: &'a PublicKey,
    ) -> LocalBoxFuture<'a, anyhow::Result<Vec<u8>>> {
        todo!()
    }
}

#[derive(Default)]
struct AliasCache {
    updated: Option<Instant>,
    data: Vec<NodeId>,
}

impl AliasCache {
    fn valid_ttl(&self) -> bool {
        const ALIAS_CACHE_TTL_SEC: Duration = Duration::from_secs(5);
        self.updated
            .map(|updated| Instant::now() - updated < ALIAS_CACHE_TTL_SEC)
            .unwrap_or(false)
    }

    fn update(&mut self, data: Vec<NodeId>) {
        self.updated.replace(Instant::now());
        self.data = data;
    }
}
