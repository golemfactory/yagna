use test_context::test_context;

use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::temp_dir;

use mock_payment::MockPayment;

mod mock_identity;
mod mock_payment;

#[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_payments_happy_path(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(true);

    let _dir = temp_dir!("test_payments_happy_path")?;

    ya_sb_router::bind_gsb_router(None).await?;
    log::debug!("bind_gsb_router()");

    let payment = MockPayment::new("payments-1");
    payment.bind_gsb().await?;
    payment.start_server(ctx, "127.0.0.1:8000").await?;

    Ok(())
}
