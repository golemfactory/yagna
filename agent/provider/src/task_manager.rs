use actix::prelude::*;
use anyhow::{Error, Result};
use chrono::{Utc, DateTime, TimeZone};
use derive_more::Display;
use futures_util::FutureExt;

use ya_utils_actix::forward_actix_handler;
use ya_utils_actix::actix_handler::ResultTypeGetter;
use ya_utils_actix::actix_signal::Subscribe;
use ya_agreement_utils::ParsedAgreement;

use crate::execution::{ActivityCreated, ActivityDestroyed, TaskRunner};
use crate::market::provider_market::{AgreementApproved, ProviderMarket};
use crate::payments::Payments;


// =========================================== //
// Messages modifying agreement state
// =========================================== //

/// These events can be send to TaskManager:
/// - AgreementApproved
/// - ActivityCreated
/// - ActivityDestroyed
/// - BreakAgreement

/// Event forces agreement termination, what includes killing ExeUnit.
/// Sending this event indicates, that agreement conditions were broken
/// somehow. Normally Requestor is responsible for agreement termination.
#[derive(Message, Clone)]
#[rtype(result = "Result<()>")]
pub struct BreakAgreement {
    pub agreement_id: String,
    pub reason: BreakAgreementReason,
}

#[derive(Display, Clone)]
pub enum BreakAgreementReason {
    Expired,
}

// =========================================== //
// Output events
// =========================================== //

/// Agreement was broken by us. All modules will get this message,
/// when TaskManager will get BreakAgreement event.
#[derive(Message, Clone)]
#[rtype(result = "Result<()>")]
pub struct AgreementBroken {
    pub agreement_id: String,
    pub reason: BreakAgreementReason,
}

/// Agreement is finished by Requestor. This is proper way to close Agreement.
#[derive(Message, Clone)]
#[rtype(result = "Result<()>")]
pub struct AgreementClosed {
    pub agreement_id: String,
}

// =========================================== //
// Agreement state
// =========================================== //

enum AgreementState {
    Approved,
    Computing,
    Closed,
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
// TaskManager internal messages
// =========================================== //

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct ScheduleExpiration(ParsedAgreement);

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

    fn schedule_expiration(
        &mut self,
        msg: ScheduleExpiration,
        ctx: &mut Context<Self>,
    ) -> Result<()> {
        let agreement_id = msg.0.agreement_id.clone();
        let expiration = agreement_expiration_from(&msg.0)?;
        let duration = (expiration - Utc::now()).to_std()?;

        // Schedule agreement termination after expiration time.
        ctx.run_later(duration, move |_, ctx| {
            let msg = BreakAgreement {
                agreement_id: agreement_id.clone(),
                reason: BreakAgreementReason::Expired,
            };
            ctx.address().do_send(msg);
        });
        Ok(())
    }
}

fn agreement_expiration_from(agreement: &ParsedAgreement) -> Result<DateTime<Utc>> {
    let expiration_key_str = "/demand/properties/golem/srv/comp/expiration";
    let timestamp = agreement.pointer_typed::<i64>(expiration_key_str)?;
    Ok(Utc.timestamp_millis(timestamp))
}

impl Actor for TaskManager {
    type Context = Context<Self>;
}

forward_actix_handler!(TaskManager, ScheduleExpiration, schedule_expiration);

impl Handler<InitializeTaskManager> for TaskManager {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, _msg: InitializeTaskManager, ctx: &mut Context<Self>) -> Self::Result {
        let self_address = ctx.address().clone();
        let runner = self.runner.clone();
        let market = self.market.clone();

        let future = async move {
            // Listen to AgreementApproved event.
            let msg = Subscribe::<AgreementApproved>(self_address.clone().recipient());
            market.send(msg).await??;

            // Get info about Activity creation and destruction.
            let msg = Subscribe::<ActivityCreated>(self_address.clone().recipient());
            runner.send(msg).await??;

            let msg = Subscribe::<ActivityDestroyed>(self_address.clone().recipient());
            Ok(runner.send(msg).await??)
        }
        .into_actor(self);

        ActorResponse::r#async(future)
    }
}

// =========================================== //
// Messages modifying agreement state
// =========================================== //

impl Handler<AgreementApproved> for TaskManager {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: AgreementApproved, ctx: &mut Context<Self>) -> Self::Result {
        let agreement_id = msg.agreement.agreement_id.clone();
        let runner = self.runner.clone();
        let payments = self.payments.clone();
        let self_address = ctx.address().clone();

        let future = async move {
            // TODO: Handle fails.
            self_address.send(ScheduleExpiration(msg.agreement.clone())).await??;
            runner.send(msg.clone()).await??;
            payments.send(msg.clone()).await??;
            Ok(())
        }
        .inspect(move |result| match result {
            Err(error) => log::error!(
                "Failed to initialize agreement [{}]. Error: {}",
                agreement_id,
                error
            ),
            Ok(_) => (),
        })
        .into_actor(self);

        ActorResponse::r#async(future)
    }
}

impl Handler<ActivityCreated> for TaskManager {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: ActivityCreated, _ctx: &mut Context<Self>) -> Self::Result {
        // Forward information to Payments for cost computing.
        self.payments.do_send(msg);
        ActorResponse::reply(Ok(()))
    }
}

impl Handler<ActivityDestroyed> for TaskManager {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: ActivityDestroyed, _ctx: &mut Context<Self>) -> Self::Result {
        let agreement_id = msg.agreement_id.clone();
        // Forward information to Payments to send last DebitNote in activity.
        self.payments.do_send(msg);

        // Temporary. Requestor should close agreement, but for now we
        // treat destroying activity as closing agreement.
        self.payments.do_send(AgreementClosed{agreement_id: agreement_id.clone()});
        self.runner.do_send(AgreementClosed{agreement_id: agreement_id.clone()});
        ActorResponse::reply(Ok(()))
    }
}

impl Handler<BreakAgreement> for TaskManager {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: BreakAgreement, _ctx: &mut Context<Self>) -> Self::Result {
        log::warn!(
            "Breaking agreement [{}], reason: {}.",
            msg.agreement_id,
            msg.reason
        );

        let runner = self.runner.clone();
        let payments = self.payments.clone();

        let future = async move {
            runner.send(AgreementBroken::from(msg.clone())).await??;
            payments.send(AgreementBroken::from(msg.clone())).await??;
            Ok(())
        };
        ActorResponse::r#async(future.into_actor(self))
    }
}

impl From<BreakAgreement> for AgreementBroken {
    fn from(msg: BreakAgreement) -> Self {
        AgreementBroken {
            agreement_id: msg.agreement_id,
            reason: msg.reason,
        }
    }
}
