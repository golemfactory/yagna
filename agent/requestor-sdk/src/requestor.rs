use crate::activity::Activity;
use crate::{payment_manager, CommandList, Image, Package};
use actix::prelude::*;
use anyhow::{Error, Result};
use bigdecimal::BigDecimal;
use futures::channel::mpsc;
use futures::future::select;
use futures::prelude::*;
use payment_manager::PaymentManager;
use std::sync::Once;
use std::{
    iter::FromIterator,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::time;
use ya_agreement_utils::{constraints, ConstraintKey, Constraints};
use ya_client::activity::ActivityRequestorApi;
use ya_client::{
    market::MarketRequestorApi,
    model::{
        self,
        activity::CommandResult,
        market::{
            proposal::{Proposal, State},
            AgreementProposal, Demand, RequestorEvent,
        },
    },
    payment::PaymentRequestorApi,
    web::WebClient,
};

const MAX_CONCURRENT_JOBS: usize = 64;
static START: Once = Once::new();

#[derive(Clone, Debug, MessageResponse)]
enum ComputationState {
    AwaitingProviders,
    AwaitingCompletion,
    Done,
}

impl Default for ComputationState {
    fn default() -> Self {
        ComputationState::AwaitingProviders
    }
}

#[derive(Clone, Debug)]
struct ComputationTracker {
    initial: usize,
    completed: usize,
}

impl Default for ComputationTracker {
    fn default() -> Self {
        ComputationTracker {
            initial: 0,
            completed: 0,
        }
    }
}

#[derive(Clone)]
pub struct Requestor {
    name: String,
    subnet: String,
    image_type: Image,
    task_package: Package,
    constraints: Constraints,
    tee: bool,
    tasks: Vec<CommandList>,
    timeout: Duration,
    budget: BigDecimal,
    state: ComputationState,
    tracker: ComputationTracker,
    on_completed: Option<Arc<dyn Fn(String, Vec<String>)>>,
}

impl Requestor {
    /// Creates a new requestor from `Image` and `Package` with given `name`.
    pub fn new<T: Into<String>>(name: T, image_type: Image, task_package: Package) -> Self {
        START.call_once(|| {
            dotenv::dotenv().ok();
            env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();
        });

        Self {
            name: name.into(),
            subnet: "testnet".into(),
            image_type,
            task_package,
            constraints: constraints!["golem.com.pricing.model" == "linear"], /* TODO: other models */
            tee: false,
            tasks: vec![],
            timeout: Duration::from_secs(60),
            budget: 0.into(),
            state: ComputationState::default(),
            tracker: ComputationTracker::default(),
            on_completed: None,
        }
    }

    /// Compute in a Trusted Execution Environment.
    pub fn tee(self) -> Self {
        Self { tee: true, ..self }
    }

    /// `Demand`s will be handled only by providers in this subnetwork.
    pub fn with_subnet(self, subnet: impl ToString) -> Self {
        Self {
            subnet: subnet.to_string(),
            ..self
        }
    }

    /// Adds `Constraints` for the specified tasks.
    pub fn with_constraints(self, constraints: Constraints) -> Self {
        Self {
            constraints: self.constraints.and(constraints),
            ..self
        }
    }

    /// Adds some `timeout` value for the tasks.
    pub fn with_timeout(self, timeout: Duration) -> Self {
        Self { timeout, ..self }
    }

    /// Sets the max budget in GNT.
    pub fn with_max_budget_ngnt<T: Into<BigDecimal>>(self, budget: T) -> Self {
        Self {
            budget: budget.into(),
            ..self
        }
    }

    /// Adds tasks from the specified iterator.
    pub fn with_tasks(mut self, tasks: impl IntoIterator<Item = CommandList>) -> Self {
        let tasks = Vec::from_iter(tasks);
        self.tracker.initial = tasks.len();
        Self { tasks, ..self }
    }

    /// Sets callback to invoke upon completion of the tasks.
    pub fn on_completed<T: Fn(String, Vec<String>) + 'static>(self, f: T) -> Self {
        Self {
            on_completed: Some(Arc::new(f)),
            ..self
        }
    }

    /// Runs all tasks asynchronously.
    pub async fn run(self) -> Result<()> {
        let client = web_client()?;
        let market_api: MarketRequestorApi = client.interface()?;
        let payment_api: PaymentRequestorApi = client.interface()?;
        let activity_api: ActivityRequestorApi = client.interface()?;

        let demand = self.create_demand().await?;
        log::debug!("demand: {}", serde_json::to_string_pretty(&demand)?);
        let allocation = payment_api
            .create_allocation(&model::payment::NewAllocation {
                address: None,
                payment_platform: None,
                total_amount: self.budget.clone(),
                timeout: None,
                make_deposit: false,
            })
            .await?;
        log::info!("allocated {} NGNT", &allocation.total_amount);
        let subscription_id = market_api.subscribe(&demand).await?;
        log::info!("subscribed to market (id: [{}])", subscription_id);

        let tee = self.tee;
        let timeout = self.timeout;
        let deadline = Instant::now() + timeout;

        let payment_manager = PaymentManager::new(payment_api.clone(), allocation).start();
        let requestor = self.start();

        let (proposal_tx, proposal_rx) = mpsc::channel(MAX_CONCURRENT_JOBS);
        Arbiter::spawn(process_events(
            requestor.clone(),
            market_api.clone(),
            subscription_id.clone(),
            demand,
            proposal_tx,
        ));

        let proposal_ctx = ProposalCtx {
            requestor: requestor.clone(),
            payment_manager: payment_manager.clone(),
            activity_api,
            market_api: market_api.clone(),
        };
        let comp_fut = proposal_rx.for_each_concurrent(MAX_CONCURRENT_JOBS, move |proposal| {
            let ctx = proposal_ctx.clone();
            let p_id = proposal.proposal_id.clone();

            async move {
                create_agreement(ctx.market_api.clone(), proposal)
                    .map_err(move |e| {
                        format!("create agreement error (proposal [{:?}]): {}", p_id, e)
                    })
                    // get task
                    .and_then(|agr_id| {
                        let task_fut = ctx.requestor.send(TakeTask);
                        async move {
                            let res = async { Ok::<_, Error>((agr_id.clone(), task_fut.await??)) }
                                .map_err(|e| format!("no tasks for agreement [{}]: {}", &agr_id, e))
                                .await?;
                            Ok(res)
                        }
                    })
                    // create activity
                    .and_then(|(agr_id, task)| {
                        let activity_fut = Activity::create(
                            ctx.activity_api.clone(),
                            agr_id.clone(),
                            task.clone(),
                            tee,
                        );
                        async move {
                            let activity = activity_fut.await.map_err(|e| {
                                format!("cannot create activity (agreement [{}]): {}", &agr_id, e)
                            })?;
                            Ok(activity)
                        }
                    })
                    // monitor activity
                    .and_then(|activity: Activity| {
                        let id = activity.activity_id.clone();
                        let task = activity.task.clone();
                        let req = ctx.requestor.clone();
                        let fut = monitor_activity(activity, ctx.payment_manager.clone()).then(
                            |result| async move {
                                match result {
                                    Ok(o) => {
                                        req.do_send(FinishTask(id, o));
                                    }
                                    Err(e) => {
                                        log::error!("activity [{}] error: {}", id, e);
                                        req.do_send(ReturnTask(task));
                                    }
                                }
                            },
                        );
                        Arbiter::spawn(fut);
                        futures::future::ok(())
                    })
                    .then(|result| async move {
                        if let Err(e) = result {
                            log::error!("activity error: {}", e);
                        }
                    })
                    .await;
            }
        });
        Arbiter::spawn(comp_fut);

        let wait_fut = async move {
            loop {
                match requestor.send(GetState).await {
                    Ok(ComputationState::Done) => {
                        log::info!("all activities finished");
                        break;
                    }
                    Err(e) => {
                        log::error!("unable to retrieve state: internal error: {:?}", e);
                        requestor.do_send(SetState(ComputationState::Done));
                        break;
                    }
                    _ => {
                        if Instant::now() > deadline {
                            log::warn!("computation timed out after {:?}s", timeout.as_secs());
                            requestor.do_send(SetState(ComputationState::Done));
                            break;
                        }
                    }
                }
                tokio::time::delay_for(Duration::from_secs(1)).await;
            }
        };

        let ctrl_c = actix_rt::signal::ctrl_c().then(|r| async move {
            match r {
                Ok(_) => Ok(log::warn!("interrupted: ctrl-c detected!")),
                Err(e) => Err(anyhow::Error::from(e)),
            }
        });
        let _ = select(wait_fut.boxed_local(), ctrl_c.boxed_local()).await;

        log::info!("waiting for payments");
        loop {
            let r = payment_manager.send(payment_manager::GetPending).await?;
            if r <= 0 {
                break;
            }
            log::info!("pending payments: {}", r);
            tokio::time::delay_for(Duration::from_secs(1)).await;
        }

        log::info!("unsubscribing from the market");
        if let Err(e) = market_api.unsubscribe(&subscription_id).await {
            log::warn!("unable to unsubscribe from the market: {}", e);
        }

        if let Err(e) = payment_manager
            .send(payment_manager::ReleaseAllocation)
            .await
        {
            log::warn!("unable to release allocation: {:?}", e);
        }

        Ok(())
    }

    async fn create_demand(&self) -> Result<Demand> {
        let (digest, url) = self.task_package.publish().await?;
        let url_with_hash = format!("hash:sha3:{}:{}", digest, url);
        let constraints = self.constraints.clone().and(constraints![
            "golem.runtime.name" == self.image_type.runtime_name(),
            "golem.runtime.version" == self.image_type.runtime_version().to_string(),
            "golem.node.debug.subnet" == self.subnet.clone(),
        ]);

        log::debug!("srv.comp.task_package: {}", url_with_hash);

        let deadline = chrono::Utc::now() + chrono::Duration::from_std(self.timeout.clone())?;
        let demand = Demand::new(
            serde_json::json!({
                "golem.node.id.name": self.name,
                "golem.node.debug.subnet": self.subnet.clone(),
                "golem.srv.comp.task_package": url_with_hash,
                "golem.srv.comp.expiration": deadline.timestamp_millis(),
            }),
            constraints.to_string(),
        );

        Ok(demand)
    }
}

async fn process_events(
    requestor: Addr<Requestor>,
    market_api: MarketRequestorApi,
    subscription_id: String,
    demand: Demand,
    mut tx: mpsc::Sender<Proposal>,
) {
    log::info!("processing market events");
    'outer: loop {
        let events = market_api
            .collect(&subscription_id, Some(2.0), Some(5))
            .await
            .map_err(|e| log::error!("error collecting market events: {}", e))
            .unwrap_or_else(|_| Vec::new());
        log::debug!("collected {} market events", events.len());

        for event in events {
            match requestor.send(GetState).await {
                Ok(ComputationState::Done) => break 'outer,
                Ok(ComputationState::AwaitingCompletion) => continue,
                Ok(ComputationState::AwaitingProviders) => (),
                Err(e) => {
                    log::error!("unable to retrieve state: {:?}", e);
                    break 'outer;
                }
            }

            match event {
                RequestorEvent::ProposalEvent {
                    event_date: _,
                    proposal,
                } => match proposal.state.as_ref().unwrap_or(&State::Initial) {
                    State::Initial => {
                        log::debug!("answering with counter proposal");
                        let bespoke_proposal = match proposal.counter_demand(demand.clone()) {
                            Ok(c) => c,
                            Err(e) => {
                                log::error!("counter demand error {}", e);
                                continue;
                            }
                        };

                        let market_api_clone = market_api.clone();
                        let subscription_id_clone = subscription_id.clone();
                        Arbiter::spawn(async move {
                            if let Err(e) = market_api_clone
                                .counter_proposal(&bespoke_proposal, &subscription_id_clone)
                                .await
                            {
                                log::error!("unable to counter proposal: {}", e);
                            }
                        });
                    }
                    State::Draft => {
                        log::debug!("draft proposal from [{:?}]", proposal.issuer_id);
                        if let Err(e) = tx.send(proposal).await {
                            log::error!("unable to process proposal: {:?}", e);
                        }
                    }
                    state => {
                        log::debug!(
                            "ignoring proposal [{:?}] from [{:?}] with state {:?}",
                            proposal.proposal_id,
                            proposal.issuer_id,
                            state
                        );
                    }
                },
                _ => log::debug!("expected ProposalEvent"),
            }
        }
    }
    log::info!("stopped processing market events");
}

async fn create_agreement(market_api: MarketRequestorApi, proposal: Proposal) -> Result<String> {
    let id = proposal.proposal_id()?;
    let issuer = proposal.issuer_id()?;
    let agreement = AgreementProposal::new(
        id.clone(),
        chrono::Utc::now() + chrono::Duration::minutes(10), /* TODO */
    );

    let agreement_id = market_api.create_agreement(&agreement).await?;
    log::info!(
        "created agreement [{}] with [{}]; confirming",
        agreement_id,
        issuer
    );
    let _ = market_api.confirm_agreement(&id).await?;
    log::info!("waiting for approval of agreement [{}]", agreement_id);

    let response = market_api.wait_for_approval(&id, Some(10.0)).await?;
    match response.trim().to_lowercase().as_str() {
        "approved" => Ok(agreement_id),
        res => Err(anyhow::anyhow!(
            "expected agreement approval, got {} instead",
            res
        )),
    }
}

async fn monitor_activity(
    activity: Activity,
    payment_manager: Addr<PaymentManager>,
) -> Result<Vec<String>> {
    let script = activity.script.clone();
    let activity_id = activity.activity_id.clone();
    let batch_id = activity.exec().await.map_err(|e| {
        anyhow::anyhow!(
            "activity [{}] error: exec script failed: {}",
            activity_id,
            e
        )
    })?;

    let mut all_res = vec![];
    loop {
        match activity.get_state().await {
            Ok(state) => match state.alive() {
                true => state,
                false => {
                    log::warn!("activity [{}] is no longer alive", activity_id);
                    break;
                }
            },
            Err(e) => {
                log::error!("activity [{}] get_state error: {}", activity_id, e);
                break;
            }
        };

        all_res = activity.get_exec_batch_results(&batch_id).await?;
        log::debug!("batch_results: {}", all_res.len());
        if let Some(last) = all_res.last() {
            if last.is_batch_finished {
                break;
            }
        }

        let delay = time::Instant::now() + Duration::from_secs(3);
        time::delay_until(delay).await;
    }

    if all_res.len() == script.num_cmds
        && all_res
            .last()
            .map(|l| l.result == CommandResult::Ok)
            .unwrap_or(false)
    {
        log::info!("activity [{}] finished", activity_id);
    } else {
        log::warn!("activity [{}] interrupted", activity_id);
    }

    let only_stdout = |txt: String| {
        match txt.starts_with("stdout: ") {
            true => match txt.find("\nstderr:") {
                Some(pos) => &txt[8..pos],
                None => &txt[8..],
            },
            false => "",
        }
        .to_string()
    };

    let output = all_res
        .into_iter()
        .enumerate()
        .filter_map(|(i, r)| match script.run_indices.contains(&i) {
            // stdout: {}\nstdout;
            true => Some(r.message.unwrap_or("".to_string())).map(only_stdout),
            false => None,
        })
        .collect::<Vec<_>>();

    let _ = payment_manager
        .send(payment_manager::AcceptAgreement {
            agreement_id: activity.agreement_id.clone(),
        })
        .await?;

    activity.destroy().await?;
    Ok(output)
}

impl Actor for Requestor {
    type Context = Context<Self>;
}

#[derive(Message)]
#[rtype(result = "ComputationState")]
struct GetState;

impl Handler<GetState> for Requestor {
    type Result = <GetState as Message>::Result;

    fn handle(&mut self, _: GetState, _: &mut Context<Self>) -> Self::Result {
        self.state.clone()
    }
}

#[derive(Message)]
#[rtype(result = "()")]
struct SetState(ComputationState);

impl Handler<SetState> for Requestor {
    type Result = <SetState as Message>::Result;

    fn handle(&mut self, msg: SetState, _: &mut Context<Self>) -> Self::Result {
        self.state = msg.0;
    }
}

#[derive(Message)]
#[rtype(result = "()")]
struct ReturnTask(CommandList);

impl Handler<ReturnTask> for Requestor {
    type Result = <ReturnTask as Message>::Result;

    fn handle(&mut self, msg: ReturnTask, _: &mut Context<Self>) -> Self::Result {
        self.tasks.push(msg.0);
        self.state = ComputationState::AwaitingProviders;
    }
}

#[derive(Message)]
#[rtype(result = "Result<CommandList>")]
struct TakeTask;

impl Handler<TakeTask> for Requestor {
    type Result = <TakeTask as Message>::Result;

    fn handle(&mut self, _: TakeTask, _: &mut Context<Self>) -> Self::Result {
        match self.tasks.pop() {
            Some(task) => {
                if self.tasks.len() == 0 {
                    self.state = ComputationState::AwaitingCompletion;
                }
                Ok(task)
            }
            None => Err(anyhow::anyhow!("no more tasks")),
        }
    }
}

#[derive(Message)]
#[rtype(result = "()")]
struct FinishTask(String, Vec<String>);

impl Handler<FinishTask> for Requestor {
    type Result = <FinishTask as Message>::Result;

    fn handle(&mut self, msg: FinishTask, _: &mut Context<Self>) -> Self::Result {
        self.tracker.completed += 1;
        log::warn!(
            "Completed {} out of {}",
            self.tracker.completed,
            self.tracker.initial
        );
        if self.tracker.completed == self.tracker.initial {
            self.state = ComputationState::Done;
        }
        if let Some(f) = &self.on_completed {
            f(msg.0, msg.1)
        }
    }
}

#[derive(Clone)]
struct ProposalCtx {
    requestor: Addr<Requestor>,
    payment_manager: Addr<PaymentManager>,
    activity_api: ActivityRequestorApi,
    market_api: MarketRequestorApi,
}

fn web_client() -> Result<WebClient> {
    let app_key = std::env::var("YAGNA_APPKEY")?;
    Ok(WebClient::builder().auth_token(&app_key).build())
}
