use actix::Addr;
use anyhow::{anyhow, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use url::Url;
use uuid::Uuid;

use ya_client_model::activity::exe_script_command::ProgressArgs;
use ya_client_model::activity::{ExeScriptCommand, State, StatePair};
use ya_core_model::activity;
use ya_exe_unit::message::{GetBatchResults, GetState, GetStateResponse, Shutdown, ShutdownReason};
use ya_exe_unit::runtime::process::RuntimeProcess;
use ya_exe_unit::{exe_unit, ExeUnit, ExeUnitConfig, FinishNotifier, RunArgs, SuperviseCli};
use ya_framework_basic::async_drop::{AsyncDroppable, DroppableTestContext};
use ya_service_bus::RpcEnvelope;

#[async_trait::async_trait]
pub trait ExeUnitExt {
    async fn exec(
        &self,
        batch_id: Option<String>,
        exe_script: Vec<ExeScriptCommand>,
    ) -> anyhow::Result<String>;

    async fn deploy(&self, progress: Option<ProgressArgs>) -> anyhow::Result<String>;
    async fn start(&self, args: Vec<String>) -> anyhow::Result<String>;

    async fn wait_for_batch(&self, batch_id: &str) -> anyhow::Result<()>;

    /// Waits until ExeUnit will be ready to receive commands.
    async fn await_init(&self) -> anyhow::Result<()>;
}

#[derive(Debug, Clone)]
pub struct ExeUnitHandle {
    pub addr: Addr<ExeUnit<RuntimeProcess>>,
    pub config: Arc<ExeUnitConfig>,
}

impl ExeUnitHandle {
    pub fn new(
        addr: Addr<ExeUnit<RuntimeProcess>>,
        config: ExeUnitConfig,
    ) -> anyhow::Result<ExeUnitHandle> {
        Ok(ExeUnitHandle {
            addr,
            config: Arc::new(config),
        })
    }

    pub async fn finish_notifier(&self) -> anyhow::Result<broadcast::Receiver<()>> {
        Ok(self.addr.send(FinishNotifier {}).await??)
    }
}

#[async_trait::async_trait]
impl AsyncDroppable for ExeUnitHandle {
    async fn async_drop(&self) {
        let finish = self.finish_notifier().await;
        self.addr
            .send(Shutdown(ShutdownReason::Finished))
            .await
            .ok();
        if let Ok(mut finish) = finish {
            finish.recv().await.ok();
        }
    }
}

pub fn exe_unit_config(
    temp_dir: &Path,
    agreement_path: &Path,
    binary: impl AsRef<Path>,
) -> ExeUnitConfig {
    ExeUnitConfig {
        args: RunArgs {
            agreement: agreement_path.to_path_buf(),
            cache_dir: temp_dir.join("cache"),
            work_dir: temp_dir.join("work"),
        },
        binary: binary.as_ref().to_path_buf(),
        runtime_args: vec![],
        supervise: SuperviseCli {
            hardware: false,
            image: false,
        },
        sec_key: None,
        requestor_pub_key: None,
        service_id: Some(Uuid::new_v4().to_simple().to_string()),
        report_url: None,
    }
}

pub async fn create_exe_unit(
    config: ExeUnitConfig,
    ctx: &mut DroppableTestContext,
) -> anyhow::Result<ExeUnitHandle> {
    if config.service_id.is_some() {
        let gsb_url = match std::env::consts::FAMILY {
            "unix" => Url::from_str(&format!(
                "unix://{}/gsb.sock",
                config.args.work_dir.display()
            ))?,
            _ => Url::from_str(&format!(
                "tcp://127.0.0.1:{}",
                portpicker::pick_unused_port().ok_or(anyhow!("No ports free"))?
            ))?,
        };

        if gsb_url.scheme() == "unix" {
            let dir = PathBuf::from_str(gsb_url.path())?
                .parent()
                .map(|path| path.to_path_buf())
                .ok_or(anyhow!("`gsb_url` unix socket has no parent directory."))?;
            fs::create_dir_all(dir)?;
        }

        // GSB takes url from this variable and we can't set it directly.
        std::env::set_var("GSB_URL", gsb_url.to_string());
        ya_sb_router::bind_gsb_router(Some(gsb_url.clone()))
            .await
            .map_err(|e| anyhow!("Error binding service bus router to '{}': {e}", &gsb_url))?;
    }

    let exe = exe_unit(config.clone()).await.unwrap();
    let handle = ExeUnitHandle::new(exe, config)?;
    ctx.register(handle.clone());
    Ok(handle)
}

#[async_trait::async_trait]
impl ExeUnitExt for ExeUnitHandle {
    async fn exec(
        &self,
        batch_id: Option<String>,
        exe_script: Vec<ExeScriptCommand>,
    ) -> anyhow::Result<String> {
        log::debug!("Executing commands: {:?}", exe_script);

        let batch_id = if let Some(batch_id) = batch_id {
            batch_id
        } else {
            hex::encode(rand::random::<[u8; 16]>())
        };

        let msg = activity::Exec {
            activity_id: self.config.service_id.clone().unwrap_or_default(),
            batch_id: batch_id.clone(),
            exe_script,
            timeout: None,
        };
        self.addr
            .send(RpcEnvelope::with_caller(String::new(), msg))
            .await
            .map_err(|e| anyhow!("Unable to execute exe script: {e:?}"))?
            .map_err(|e| anyhow!("Unable to execute exe script: {e:?}"))?;
        Ok(batch_id)
    }

    async fn deploy(&self, progress: Option<ProgressArgs>) -> anyhow::Result<String> {
        Ok(self
            .exec(
                None,
                vec![ExeScriptCommand::Deploy {
                    net: vec![],
                    progress,
                    env: Default::default(),
                    hosts: Default::default(),
                    hostname: None,
                    volumes: vec![],
                }],
            )
            .await
            .unwrap())
    }

    async fn start(&self, args: Vec<String>) -> anyhow::Result<String> {
        Ok(self
            .exec(None, vec![ExeScriptCommand::Start { args }])
            .await
            .unwrap())
    }

    async fn wait_for_batch(&self, batch_id: &str) -> anyhow::Result<()> {
        let delay = Duration::from_secs_f32(0.5);
        loop {
            match self
                .addr
                .send(GetBatchResults {
                    batch_id: batch_id.to_string(),
                    idx: None,
                })
                .await
            {
                Ok(results) => {
                    if let Some(last) = results.0.last() {
                        if last.is_batch_finished {
                            return Ok(());
                        }
                    }
                }
                Err(e) => bail!("Waiting for batch: {batch_id}. Error: {e}"),
            }
            tokio::time::sleep(delay).await;
        }
    }

    async fn await_init(&self) -> anyhow::Result<()> {
        let delay = Duration::from_secs_f32(0.3);
        loop {
            match self.addr.send(GetState).await {
                Ok(GetStateResponse(StatePair(State::Initialized, None))) => break,
                Ok(GetStateResponse(StatePair(State::Terminated, _)))
                | Ok(GetStateResponse(StatePair(_, Some(State::Terminated))))
                | Err(_) => {
                    log::error!("ExeUnit has terminated");
                    bail!("ExeUnit has terminated");
                }
                _ => tokio::time::sleep(delay).await,
            }
        }
        Ok(())
    }
}
