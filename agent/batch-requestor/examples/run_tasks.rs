use std::time::Duration;
use ya_batch_requestor::{
    command_helpers::*, commands, tui_progress_monitor, CommandList, ImageSpec, TaskSession,
    WasmDemand, WasmRuntime,
};

#[actix_rt::main]
async fn main() -> Result<(), ()> {
    let tasks = vec!["1", "2", "3", "4", "5"];
    let image_spec =
        ImageSpec::from_github("prekucki/test-wasi@0.1.0").runtime(WasmRuntime::Wasi(1));

    let batch_req_addr = TaskSession::new("simple wasm app")
        .with_timeout(Duration::from_secs(60))
        .demand(
            WasmDemand::with_image(image_spec)
                .min_ram_gib(0.5)
                .min_storage_gib(1.0),
        )
        .tasks(tasks.into_iter().map(|arg| {
            commands! {
                deploy;
                start;
                run(&["abc", arg]);
                stop;
            }
        }))
        .run();

    tui_progress_monitor(batch_req_addr).await;
    Ok(())
}
