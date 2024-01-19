use actix::Addr;
use test_context::test_context;
use ya_client_model::activity::ExeScriptCommand;
use ya_exe_unit::message::{Shutdown, ShutdownReason};
use ya_exe_unit::runtime::process::RuntimeProcess;
use ya_exe_unit::{
    exe_unit, send_script, ExeUnit, ExeUnitConfig, FinishNotifier, RunArgs, SuperviseCli,
};
use ya_framework_basic::async_drop::{AsyncDroppable, DroppableTestContext};
use ya_framework_basic::file::generate_file_with_hash;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::server_external::start_http;
use ya_framework_basic::test_dirs::cargo_binary;
use ya_framework_basic::{resource, temp_dir};

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

#[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_exe_unit_start_terminate(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(true);

    let dir = temp_dir!("exe-unit-start-terminate")?;
    let temp_dir = dir.path();

    generate_file_with_hash(temp_dir, "rnd", 4096_usize, 10);
    start_http(ctx, temp_dir.to_path_buf())
        .await
        .expect("unable to start http servers");

    let config = ExeUnitConfig {
        args: RunArgs {
            agreement: resource!("agreement.json"),
            cache_dir: temp_dir.join("cache"),
            work_dir: temp_dir.join("work"),
        },
        binary: cargo_binary("ya-mock-runtime").unwrap(),
        runtime_args: vec![],
        supervise: SuperviseCli {
            hardware: false,
            image: false,
        },
        sec_key: None,
        requestor_pub_key: None,
        service_id: None,
        report_url: None,
    };

    let exe = exe_unit(config).await.unwrap();
    let mut finish = exe.send(FinishNotifier {}).await??;
    ctx.register(ExeUnitHandle(exe.clone()));

    log::info!("Sending [deploy, start] batch for execution.");

    send_script(
        exe.clone(),
        None,
        vec![
            ExeScriptCommand::Deploy {
                net: vec![],
                progress: None,
                env: Default::default(),
                hosts: Default::default(),
                hostname: None,
                volumes: vec![],
            },
            ExeScriptCommand::Start { args: vec![] },
        ],
    )
    .await;

    log::info!("Sending shutdown request.");

    exe.send(Shutdown(ShutdownReason::Finished))
        .await
        .unwrap()
        .unwrap();

    finish.recv().await.unwrap();

    Ok(())
}
