use bigdecimal::BigDecimal;
use chrono::Utc;
use test_context::test_context;

use ya_client_model::payment::allocation::{PaymentPlatform, PaymentPlatformEnum};
use ya_client_model::payment::{Acceptance, NewAllocation, NewInvoice};
use ya_core_model::payment::local::GetStatus;
use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::{resource, temp_dir};
use ya_framework_mocks::market::FakeMarket;
use ya_framework_mocks::net::MockNet;
use ya_framework_mocks::node::MockNode;
use ya_framework_mocks::payment::Driver;

#[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_release_allocation(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_release_allocation")?;

    let net = MockNet::new().bind();

    let node = MockNode::new(net, "node-1", dir.path())
        .with_identity()
        .with_payment()
        .with_fake_market();
    node.bind_gsb(false).await?;
    node.start_server(ctx).await?;

    let requestor_appkey = node
        .get_identity()?
        .create_from_private_key(&resource!("ci-requestor-1.key.priv"))
        .await?;
    let provider_appkey = node.get_identity()?.create_identity_key("provider").await?;

    let provider = node.rest_payments(&provider_appkey.key)?;
    let requestor = node.rest_payments(&requestor_appkey.key)?;

    node.get_payment()?
        .fund_account(Driver::Erc20, &requestor_appkey.identity.to_string())
        .await?;

    let payment_platform =
        PaymentPlatformEnum::PaymentPlatformName("erc20-holesky-tglm".to_string());

    log::info!("Creating allocation...");
    let allocation = requestor
        .create_allocation(&NewAllocation {
            address: Some(requestor_appkey.identity.to_string()),
            payment_platform: Some(payment_platform.clone()),
            total_amount: BigDecimal::from(10u64),
            timeout: None,
            make_deposit: false,
            deposit: None,
            extend_timeout: None,
        })
        .await?;
    log::info!("Allocation created.");

    log::info!("Creating fake Agreement...");

    let agreement =
        FakeMarket::create_fake_agreement(requestor_appkey.identity, provider_appkey.identity)
            .unwrap();
    node.get_market()?.add_agreement(agreement.clone()).await;

    log::info!("Fake Agreement created: {}", agreement.agreement_id);

    log::info!("Verifying allocation...");
    let allocations = requestor.get_allocations::<Utc>(None, None).await?;
    assert_eq!(allocations.len(), 1);
    assert_eq!(allocations[0], allocation);
    let allocation1 = requestor.get_allocation(&allocation.allocation_id).await?;
    assert_eq!(allocation1, allocation);
    log::info!("Done.");

    log::info!("Releasing allocation...");
    requestor
        .release_allocation(&allocation.allocation_id)
        .await?;
    log::info!("Allocation released.");

    log::info!("Verifying allocation removal...");
    let allocations = requestor.get_allocations::<Utc>(None, None).await?;
    assert_eq!(allocations.len(), 0);
    let result = requestor.get_allocation(&allocation.allocation_id).await;
    assert!(result.is_err());
    log::info!("Done. (Verifying allocation removal)");

    log::info!("Issuing invoice...");
    let invoice = provider
        .issue_invoice(&NewInvoice {
            agreement_id: agreement.agreement_id.clone(),
            activity_ids: None,
            amount: BigDecimal::from(1u64),
            payment_due_date: Utc::now(),
        })
        .await?;
    log::info!("Invoice issued.");

    log::info!("Sending invoice...");
    provider.send_invoice(&invoice.invoice_id).await?;
    log::info!("Invoice sent.");

    log::info!("Attempting to accept invoice...");
    let result = requestor
        .accept_invoice(
            &invoice.invoice_id,
            &Acceptance {
                total_amount_accepted: invoice.amount.clone(),
                allocation_id: allocation.allocation_id,
            },
        )
        .await;
    assert!(result.is_err());
    log::info!("Failed to accept invoice (as expected).");

    log::info!("Creating another allocation...");
    let allocation = requestor
        .create_allocation(&NewAllocation {
            address: Some(requestor_appkey.identity.to_string()),
            payment_platform: Some(payment_platform.clone()),
            total_amount: BigDecimal::from(10u64),
            timeout: None,
            make_deposit: false,
            deposit: None,
            extend_timeout: None,
        })
        .await?;
    log::info!("Allocation created.");

    log::info!("Accepting invoice...");
    requestor
        .accept_invoice(
            &invoice.invoice_id,
            &Acceptance {
                total_amount_accepted: invoice.amount.clone(),
                allocation_id: allocation.allocation_id.clone(),
            },
        )
        .await?;
    log::info!("Invoice accepted.");

    log::info!("Releasing allocation...");
    requestor
        .release_allocation(&allocation.allocation_id)
        .await?;
    log::info!("Allocation released.");

    log::info!("Verifying allocation removal...");
    let allocations = requestor.get_allocations::<Utc>(None, None).await?;
    assert_eq!(allocations.len(), 0);
    let result = requestor.get_allocation(&allocation.allocation_id).await;
    assert!(result.is_err());
    log::info!("Done.");

    log::info!(" 👍🏻 Example completed successfully ❤️");
    Ok(())
}

#[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_validate_allocation(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_validate_allocation")?;

    let net = MockNet::new().bind();

    let node = MockNode::new(net, "node-1", dir.path())
        .with_identity()
        .with_payment()
        .with_fake_market();
    node.bind_gsb(false).await?;
    node.start_server(ctx).await?;

    let appkey_req = node
        .get_identity()?
        .create_from_private_key(&resource!("ci-requestor-1.key.priv"))
        .await?;

    let requestor = node.rest_payments(&appkey_req.key)?;

    let payment = node.get_payment()?;
    payment
        .fund_account(Driver::Erc20, &appkey_req.identity.to_string())
        .await?;

    let payment_platform = PaymentPlatform {
        driver: Some(Driver::Erc20.gsb_name()),
        network: Some("holesky".to_string()),
        token: Some("tglm".to_string()),
    };

    let status = payment
        .gsb_local_endpoint()
        .call(GetStatus {
            address: appkey_req.identity.to_string(),
            driver: payment_platform.driver.clone().unwrap(),
            network: payment_platform.network.clone(),
            token: None,
            after_timestamp: 0,
        })
        .await??;

    log::info!(
        "Requestor balance: {}, platform: {:?}",
        status.amount,
        payment_platform
    );

    log::info!("Attempting to create allocation with invalid address...");
    let result = requestor
        .create_allocation(&NewAllocation {
            address: Some("Definitely not a valid address".to_string()),
            payment_platform: Some(PaymentPlatformEnum::PaymentPlatform(
                payment_platform.clone(),
            )),
            total_amount: BigDecimal::from(1u64),
            timeout: None,
            make_deposit: false,
            deposit: None,
            extend_timeout: None,
        })
        .await;
    assert!(result.is_err());
    log::info!("Failed to create allocation (as expected).");

    let new_allocation = NewAllocation {
        address: None, // Use default address (i.e. identity)
        payment_platform: Some(PaymentPlatformEnum::PaymentPlatform(
            payment_platform.clone(),
        )),
        total_amount: status.amount / 2,
        timeout: None,
        make_deposit: false,
        deposit: None,
        extend_timeout: None,
    };

    log::info!(
        "Creating allocation for {} tGLM...",
        &new_allocation.total_amount
    );
    requestor.create_allocation(&new_allocation).await?;
    log::info!("Allocation created.");

    log::info!(
        "Creating another allocation for {} tGLM...",
        &new_allocation.total_amount
    );
    let allocation = requestor.create_allocation(&new_allocation).await?;
    log::info!("Allocation created.");

    log::info!(
        "Attempting to create another allocation for {} tGLM...",
        &new_allocation.total_amount
    );
    let result = requestor.create_allocation(&new_allocation).await;
    assert!(result.is_err());
    log::info!("Failed to create allocation (as expected).");

    log::info!("Releasing an allocation...");
    requestor
        .release_allocation(&allocation.allocation_id)
        .await?;
    log::info!("Allocation released.");

    log::info!(
        "Creating another allocation for {} tGLM...",
        &new_allocation.total_amount
    );
    requestor.create_allocation(&new_allocation).await?;
    log::info!("Allocation created.");

    log::info!(" 👍🏻 Example completed successfully ❤️");
    Ok(())
}
