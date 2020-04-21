use actix::prelude::*;
use anyhow::{Result, Error};
use derive_more::Display;
use futures_util::FutureExt;

use ya_utils_actix::actix_signal::Subscribe;

use crate::execution::{TaskRunner, ActivityCreated, ActivityDestroyed};
use crate::market::provider_market::{AgreementApproved, ProviderMarket};
use crate::payments::Payments;

// =========================================== //
// Messages modifying agreement state
// =========================================== //

/// Input events:
/// - AgreementApproved
/// - ActivityCreated
/// - ActivityDestroyed
/// - BreakAgreement

/// Event forces agreement termination, what includes killing ExeUnit.
/// Sending this event indicates, that agreement conditions were broken
/// somehow. Normally Requestor is responsible for agreement termination.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct BreakAgreement {
    pub agreement_id: String,
    pub reason: BreakAgreementReason,
}

#[derive(Display)]
pub enum BreakAgreementReason {
    Expired,
}

// =========================================== //
// Agreement state
// =========================================== //

enum AgreementState {
    Approved,
    Computing,
    Finished,
    Failed,
    Broken,
}

// =========================================== //
// TaskManager messages not related to agreements
// =========================================== //

/// Initialize TaskManager.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct InitializeTaskManager;

// =========================================== //
// TaskManager implementation
// =========================================== //

/// Task manager is responsible for managing tasks (agreements)
/// state. It controls whole flow of task execution from the point,
/// when it gets signed agreement from market, to the point of agreement payment.
pub struct TaskManager {
    market: Addr<ProviderMarket>,
    runner: Addr<TaskRunner>,
    payments: Addr<Payments>,
}

impl TaskManager {
    pub fn new(
        market: Addr<ProviderMarket>,
        runner: Addr<TaskRunner>,
        payments: Addr<Payments>,
    ) -> Result<TaskManager> {
        Ok(TaskManager {
            market,
            runner,
            payments,
        })
    }
}

impl Actor for TaskManager {
    type Context = Context<Self>;
}

impl Handler<InitializeTaskManager> for TaskManager {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, _msg: InitializeTaskManager, ctx: &mut Context<Self>) -> Self::Result {
        let my_address = ctx.address().clone();
        let runner = self.runner.clone();
        let market = self.market.clone();

        let future = async move {
            // Listen to AgreementApproved event.
            let msg = Subscribe::<AgreementApproved>(my_address.clone().recipient());
            market.send(msg).await??;

            // Get info about Activity creation and destruction.
            let msg = Subscribe::<ActivityCreated>(my_address.clone().recipient());
            runner.send(msg).await??;

            let msg = Subscribe::<ActivityDestroyed>(my_address.clone().recipient());
            Ok(runner.send(msg).await??)
        }.into_actor(self);

        ActorResponse::r#async(future)
    }
}

// =========================================== //
// Messages modifying agreement state
// =========================================== //

impl Handler<AgreementApproved> for TaskManager {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: AgreementApproved, _ctx: &mut Context<Self>) -> Self::Result {
        let agreement_id = msg.agreement.agreement_id.clone();
        let runner = self.runner.clone();
        let payments = self.payments.clone();

        let future = async move {
            // TODO: Handle fails. This function preserves previous code behavior.
            runner.send(msg.clone()).await??;
            Ok(payments.send(msg.clone()).await??)
        }
        .inspect(move |result| match result {
            Err(error) => log::error!("Failed to initialize agreement [{}]. Error: {}", agreement_id, error),
            Ok(_) => ()
        }).into_actor(self);

        ActorResponse::r#async(future)
    }
}

impl Handler<ActivityCreated> for TaskManager {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: ActivityCreated, _ctx: &mut Context<Self>) -> Self::Result {
        self.payments.do_send(msg);
        ActorResponse::reply(Ok(()))
    }
}

impl Handler<ActivityDestroyed> for TaskManager {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: ActivityDestroyed, _ctx: &mut Context<Self>) -> Self::Result {
        self.payments.do_send(msg);
        ActorResponse::reply(Ok(()))
    }
}

impl Handler<BreakAgreement> for TaskManager {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: BreakAgreement, ctx: &mut Context<Self>) -> Self::Result {
        ActorResponse::reply(Ok(()))
    }
}
