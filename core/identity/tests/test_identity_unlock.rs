use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use test_context::test_context;

use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::temp_dir;
use ya_framework_mocks::net::MockNet;
use ya_framework_mocks::node::MockNode;

#[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_identity_unlock(_ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_identity_unlock")?;
    let dir = dir.path();

    let net = MockNet::new().bind();

    let node1 = MockNode::new(net.clone(), "node-1", dir)
        .with_prefixed_gsb()
        .with_identity();
    node1.bind_gsb().await?;

    // Create new identity
    let identity = node1.get_identity()?;
    let appkey = identity.create_identity_key("locked-id").await?;

    // Lock identity and change it to default. After restart this identity needs to be unlocked
    // for identity modules to start.
    let password = "password1234";
    identity.set_default_identity(appkey.identity).await?;
    identity.lock_identity(appkey.identity, password).await?;

    node1.stop().await;

    let started = Arc::new(AtomicBool::new(false));
    let started_ = started.clone();

    tokio::task::spawn_local(async move {
        node1.bind_gsb().await.unwrap();
        log::info!("Finished starting node.");
        started_.store(true, Ordering::SeqCst);
    });

    assert!(!started.load(Ordering::SeqCst));

    // Make sure that Identity::bind is still waiting for the identity to be unlocked.
    tokio::time::sleep(Duration::from_secs(3)).await;

    identity.unlock_identity(appkey.identity, password).await?;

    // Now Identity module should be able to start.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(started.load(Ordering::SeqCst));
    Ok(())
}
