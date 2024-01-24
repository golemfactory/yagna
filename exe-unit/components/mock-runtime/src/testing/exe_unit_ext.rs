use actix::Addr;
use anyhow::{anyhow, bail};
use std::path::Path;
use std::time::Duration;

use ya_client_model::activity::ExeScriptCommand;
use ya_core_model::activity;
use ya_exe_unit::message::{GetBatchResults, Shutdown, ShutdownReason};
use ya_exe_unit::runtime::process::RuntimeProcess;
use ya_exe_unit::{ExeUnit, ExeUnitConfig, FinishNotifier, RunArgs, SuperviseCli};
use ya_framework_basic::async_drop::AsyncDroppable;
use ya_service_bus::RpcEnvelope;

#[async_trait::async_trait]
pub trait ExeUnitExt {
    async fn exec(
        &self,
        activity_id: Option<String>,
        exe_script: Vec<ExeScriptCommand>,
    ) -> anyhow::Result<String>;
    async fn wait_for_batch(&self, batch_id: &str) -> anyhow::Result<()>;
}

#[derive(Debug, Clone)]
pub struct ExeUnitHandle(pub Addr<ExeUnit<RuntimeProcess>>);

#[async_trait::async_trait]
impl AsyncDroppable for ExeUnitHandle {
    async fn async_drop(&self) {
        let finish = self.0.send(FinishNotifier {}).await;
        self.0.send(Shutdown(ShutdownReason::Finished)).await.ok();
        if let Ok(Ok(mut finish)) = finish {
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
        service_id: None,
        report_url: None,
    }
}

#[async_trait::async_trait]
impl ExeUnitExt for Addr<ExeUnit<RuntimeProcess>> {
    async fn exec(
        &self,
        activity_id: Option<String>,
        exe_script: Vec<ExeScriptCommand>,
    ) -> anyhow::Result<String> {
        log::debug!("Executing commands: {:?}", exe_script);

        let batch_id = hex::encode(rand::random::<[u8; 16]>());
        let msg = activity::Exec {
            activity_id: activity_id.unwrap_or_default(),
            batch_id: batch_id.clone(),
            exe_script,
            timeout: None,
        };
        self.send(RpcEnvelope::with_caller(String::new(), msg))
            .await
            .map_err(|e| anyhow!("Unable to execute exe script: {e:?}"))?
            .map_err(|e| anyhow!("Unable to execute exe script: {e:?}"))?;
        Ok(batch_id)
    }

    async fn wait_for_batch(&self, batch_id: &str) -> anyhow::Result<()> {
        let delay = Duration::from_secs_f32(0.5);
        loop {
            match self
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
}
