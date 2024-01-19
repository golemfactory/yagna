use actix::Addr;
use anyhow::bail;
use std::path::Path;
use std::time::Duration;

use ya_exe_unit::message::{GetBatchResults, Shutdown, ShutdownReason};
use ya_exe_unit::runtime::process::RuntimeProcess;
use ya_exe_unit::{ExeUnit, ExeUnitConfig, FinishNotifier, RunArgs, SuperviseCli};
use ya_framework_basic::async_drop::AsyncDroppable;
use ya_framework_basic::test_dirs::cargo_binary;

#[async_trait::async_trait]
pub trait ExeUnitExt {
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

pub fn exe_unit_config(temp_dir: &Path, agreement_path: &Path, binary: &str) -> ExeUnitConfig {
    ExeUnitConfig {
        args: RunArgs {
            agreement: agreement_path.to_path_buf(),
            cache_dir: temp_dir.join("cache"),
            work_dir: temp_dir.join("work"),
        },
        binary: cargo_binary(binary).unwrap(),
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
