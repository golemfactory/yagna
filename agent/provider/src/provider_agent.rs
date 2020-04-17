use actix::prelude::*;
use actix::utils::IntervalFunc;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ya_agent_offer_model::{InfNodeInfo, NodeInfo, OfferDefinition, ServiceInfo};
use ya_utils_actix::{actix_handler::send_message, actix_signal::Subscribe};

use crate::execution::{
    ActivityCreated, ActivityDestroyed, InitializeExeUnits, TaskRunner, UpdateActivity,
};
use crate::market::{
    provider_market::{AgreementApproved, OnShutdown, UpdateMarket},
    CreateOffer, ProviderMarket,
};
use crate::payments::Payments;
use crate::startup_config::StartupConfig;

pub struct ProviderAgent {
    market: Addr<ProviderMarket>,
    runner: Addr<TaskRunner>,
    payments: Addr<Payments>,
    node_info: NodeInfo,
    service_info: ServiceInfo,
    exe_unit_path: String,
}

impl ProviderAgent {
    pub async fn new(config: StartupConfig) -> anyhow::Result<ProviderAgent> {
        let market = ProviderMarket::new(config.market_client()?, "AcceptAll").start();
        let runner = TaskRunner::new(config.activity_client()?)?.start();
        let payments = Payments::new(
            config.activity_client()?,
            config.payment_client()?,
            &config.credit_address,
        )
        .start();

        let node_info = ProviderAgent::create_node_info();
        let service_info = ProviderAgent::create_service_info();

        let mut provider = ProviderAgent {
            market,
            runner,
            payments,
            node_info,
            service_info,
            exe_unit_path: config.exe_unit_path,
        };
        provider.initialize().await?;

        Ok(provider)
    }

    pub async fn initialize(&mut self) -> anyhow::Result<()> {
        // Forward AgreementApproved event to TaskRunner actor.
        let msg = Subscribe::<AgreementApproved>(self.runner.clone().recipient());
        self.market.send(msg).await??;

        let msg = Subscribe::<AgreementApproved>(self.payments.clone().recipient());
        self.market.send(msg).await??;

        //
        let msg = Subscribe::<ActivityCreated>(self.payments.clone().recipient());
        self.runner.send(msg).await??;

        let msg = Subscribe::<ActivityDestroyed>(self.payments.clone().recipient());
        self.runner.send(msg).await??;

        // Load ExeUnits descriptors from file.
        fn expand_filename(exeunit_path: &str) -> anyhow::Result<Vec<PathBuf>> {
            use std::fs::read_dir;

            let path: &Path = exeunit_path.as_ref();
            let (base_dir, file_name) = match (path.parent(), path.file_name()) {
                (Some(base_dir), Some(file_name)) => (base_dir, file_name),
                _ => return Ok(vec![PathBuf::from(exeunit_path)]),
            };
            let file_name = match file_name.to_str() {
                Some(f) => f,
                None => anyhow::bail!("not utf-8 filename"),
            };

            if let Some(pos) = file_name.find("*") {
                let (prefix, suffix) = file_name.split_at(pos);
                let suffix = &suffix[1..];

                Ok(read_dir(base_dir)?
                    .filter_map(|ent| {
                        let ent = ent.ok()?;
                        let os_file_name = ent.file_name();
                        let file_name = os_file_name.to_str()?;
                        if file_name.starts_with(prefix) && file_name.ends_with(suffix) {
                            Some(ent.path())
                        } else {
                            None
                        }
                    })
                    .collect())
            } else {
                Ok(vec![PathBuf::from(exeunit_path)])
            }
        }

        for file in expand_filename(&self.exe_unit_path)? {
            let msg = InitializeExeUnits { file };
            self.runner.send(msg).await??;
        }

        // Create simple offer on market.
        let create_offer_message = CreateOffer {
            offer_definition: OfferDefinition {
                node_info: self.node_info.clone(),
                service: self.service_info.clone(),
                com_info: Default::default(),
            },
        };
        Ok(self.market.send(create_offer_message).await??)
    }

    fn schedule_jobs(&mut self, _ctx: &mut Context<Self>) {
        send_message(self.runner.clone(), UpdateActivity);
        send_message(self.market.clone(), UpdateMarket);
    }

    fn create_node_info() -> NodeInfo {
        // TODO: Get node name from intentity API.
        NodeInfo::with_name("")
    }

    fn create_service_info() -> ServiceInfo {
        let inf = InfNodeInfo::new().with_mem(1.0).with_storage(10.0);
        let wasi_version = "0.0.0".into();
        ServiceInfo::Wasm { inf, wasi_version }
    }

    pub async fn wait_for_ctrl_c(self) -> anyhow::Result<()> {
        let market = self.market.clone();

        self.start();

        let _ = tokio::signal::ctrl_c().await;
        println!();
        log::info!(
            "SIGINT received, Shutting down {}...",
            structopt::clap::crate_name!()
        );

        market.send(OnShutdown {}).await?
    }
}

impl Actor for ProviderAgent {
    type Context = Context<Self>;

    fn started(&mut self, context: &mut Context<Self>) {
        IntervalFunc::new(Duration::from_secs(4), Self::schedule_jobs)
            .finish()
            .spawn(context);
    }
}
