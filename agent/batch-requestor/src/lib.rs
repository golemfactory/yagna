use actix::prelude::*;
use chrono::Utc;
use futures::prelude::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::{str::FromStr, time::Duration};
use url::Url;
use ya_agreement_utils::{constraints, ConstraintKey, Constraints};
use ya_client::{
    activity::ActivityRequestorControlApi,
    market::MarketRequestorApi,
    model::market::{
        proposal::State,
        {AgreementProposal, Demand, Proposal, RequestorEvent},
    },
    web::WebClient,
};

#[derive(Clone)]
pub enum WasmRuntime {
    Wasi(i32), /* Wasi version */
}

#[derive(Clone)]
pub struct ImageSpec {
    runtime: WasmRuntime,
    /* TODO */
}

impl ImageSpec {
    pub fn from_github<T: Into<String>>(_github_repository: T) -> Self {
        Self {
            runtime: WasmRuntime::Wasi(1),
        }
        /* TODO connect and download image specification */
    }
    pub fn from_url<T: Into<String>>(url: T) -> Self {
        Self {
            runtime: WasmRuntime::Wasi(1),
        }
    }
    pub fn runtime(self, runtime: WasmRuntime) -> Self {
        Self { runtime }
    }
}

pub enum Command {
    Deploy,
    Start,
    Run(Vec<String>),
    Stop,
}

pub struct CommandList(Vec<Command>);

impl CommandList {
    pub fn new(v: Vec<Command>) -> Self {
        Self(v)
    }
}

pub struct TaskSession {
    name: String,
    timeout: Duration,
    demand: Option<WasmDemand>,
    tasks: Vec<CommandList>,
}

impl TaskSession {
    pub fn new<T: Into<String>>(name: T) -> Self {
        Self {
            name: name.into(),
            timeout: Duration::from_secs(60),
            demand: None,
            tasks: vec![],
        }
    }
    pub fn with_timeout(self, timeout: std::time::Duration) -> Self {
        Self { timeout, ..self }
    }
    pub fn demand(self, demand: WasmDemand) -> Self {
        Self {
            demand: Some(demand),
            ..self
        }
    }
    pub fn tasks<T: std::iter::Iterator<Item = CommandList>>(self, tasks: T) -> Self {
        Self {
            tasks: tasks.collect(),
            ..self
        }
    }
    pub fn run(self) -> Addr<TaskSession> {
        self.start()
    }
}

#[derive(Clone)]
pub struct WasmDemand {
    spec: ImageSpec,
    min_ram_gib: f64,
    min_storage_gib: f64,
}

impl WasmDemand {
    pub fn with_image(spec: ImageSpec) -> Self {
        Self {
            spec,
            min_ram_gib: 0.0,
            min_storage_gib: 0.0,
        }
    }
    pub fn min_ram_gib<T: Into<f64>>(self, min_ram_gib: T) -> Self {
        Self {
            min_ram_gib: min_ram_gib.into(),
            ..self
        }
    }
    pub fn min_storage_gib<T: Into<f64>>(self, min_storage_gib: T) -> Self {
        Self {
            min_storage_gib: min_storage_gib.into(),
            ..self
        }
    }
}

impl From<WasmDemand> for Demand {
    fn from(wasm_demand: WasmDemand) -> Self {
        Demand::new(
            serde_json::json!({
                "golem": {
                    "node": {
                        "id": {
                            "name": "xyz"
                        },
                        "ala": 1
                    }
                }
            }),
            constraints![
                "golem.inf.mem.gib" > wasm_demand.min_ram_gib,
                "golem.inf.storage.gib" > wasm_demand.min_storage_gib,
            ]
            .to_string(),
        )
    }
}

#[macro_export]
macro_rules! expand_cmd {
    (deploy) => { ya_batch_requestor::Command::Deploy };
    (start) => { ya_batch_requestor::Command::Start };
    (stop) => { ya_batch_requestor::Command::Stop };
    (run ( $($e:expr)* )) => {{
        ya_batch_requestor::Command::Run(vec![ $($e.to_string()),* ])
    }};
}

#[macro_export]
macro_rules! commands_helper {
    () => {};
    ( $i:ident ( $($param:expr),* ) $(;)* ) => {{
        vec![$crate::expand_cmd!($i ( $($param)* ))]
    }};
    ( $i:tt $(;)* ) => {{
        vec![$crate::expand_cmd!($i)]
    }};
    ( $i:ident ( $($param:expr),* ) ; $( $t:tt )* ) => {{
        let mut tail = $crate::commands_helper!( $($t)* );
        tail.push($crate::expand_cmd!($i ( $($param)* )));
        tail
    }};
    ( $i:tt ; $( $t:tt )* ) => {{
        let mut tail = $crate::commands_helper!( $($t)* );
        tail.push($crate::expand_cmd!($i));
        tail
    }};
}

#[macro_export]
macro_rules! commands {
    ( $( $t:tt )* ) => {{
        let mut v = $crate::commands_helper!( $($t)* );
        v.reverse();
        CommandList::new(v)
    }};
}

impl Actor for TaskSession {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        /* TODO 1. app key 2. URLs from env */
        let app_key = "TODO app key";
        let market_url = Url::from_str("http://34.244.4.185:8080/market-api/v1/").unwrap();
        let activity_url = Url::from_str("http://127.0.0.1:7465/activity-api/v1/").unwrap();
        let market_api: MarketRequestorApi = WebClient::with_token(app_key)
            .unwrap()
            .interface_at(market_url)
            .unwrap();
        let activity_api: ActivityRequestorControlApi = WebClient::with_token(app_key)
            .unwrap()
            .interface_at(activity_url)
            .unwrap();

        let demand: Demand = self.demand.clone().unwrap().into();
        /* TODO 1. download image spec (demand.spec) 2. market api -> subscribe 3. activity_api */

        eprintln!(
            "Actor started. Demand: {}",
            serde_json::to_string(&demand).unwrap()
        );
        ctx.spawn(
            async move {
                /* TODO */
                eprintln!("subscribing");
                let sub_id = market_api.subscribe(&demand).await?;
                eprintln!("subscription result: {:?}", sub_id);
                loop {
                    eprintln!("waiting");
                    match market_api.collect(&sub_id, Some(120.0), Some(5)).await {
                        Ok(events) => {
                            eprintln!("received {:?}", events);
                            for e in events {
                                match e {
                                    RequestorEvent::ProposalEvent {
                                        event_date: _date,
                                        proposal,
                                    } => match proposal.state {
                                        None | Some(State::Initial) => {
                                            let counter_proposal =
                                                proposal.counter_demand(demand.clone())?;
                                            /* TODO spawn? */
                                            let proposal_id = market_api
                                                .counter_proposal(&counter_proposal, &sub_id)
                                                .await?;
                                        }
                                        _ => {
                                            let agreement_proposal = AgreementProposal::new(
                                                proposal.proposal_id()?.clone(),
                                                Utc::now() + chrono::Duration::hours(2),
                                            );
                                            let agreement_id = market_api
                                                .create_agreement(&agreement_proposal)
                                                .await?;
                                            /* TODO allocate funds here using create_allocation and cancel_agreement if it's not possible */
                                            market_api
                                                .confirm_agreement(&proposal.proposal_id()?)
                                                .await?;
                                            /* blocking? */
                                            market_api.wait_for_approval(&proposal.proposal_id()?, Some(7.879))
                                                .await?;
                                            /* agreement approved; proceed to the next step (payment, then exe unit) */
                                        }
                                    },
                                    _ => { /* TODO expected proposal event, got other event */ }
                                }
                            }
                        }
                        Err(e) => eprintln!("error {:?}", e),
                    }
                    tokio::time::delay_for(Duration::from_millis(1000)).await;
                }
                Ok::<(), ya_client::error::Error>(())
            }
            .into_actor(self)
            .then(|result, ctx, _| {
                eprintln!("Received result {:?}", result);
                fut::ready(())
            }),
        );
        eprintln!("done");
    }
}

struct GetStatus {}

impl Message for GetStatus {
    type Result = f32;
}

impl Handler<GetStatus> for TaskSession {
    type Result = f32;

    fn handle(&mut self, msg: GetStatus, ctx: &mut Self::Context) -> Self::Result {
        unimplemented!()
    }
}

pub async fn tui_progress_monitor(task_session: Addr<TaskSession>) -> Result<(), ()> {
    /* TODO attach to the actor */
    let progress_bar = ProgressBar::new(100);
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .progress_chars("=> ")
            .template("{elapsed_precise} [{bar:40}] {msg}"),
    );
    //progress_bar.set_message("Running tasks");
    for _ in 0..100 {
        //progress_bar.inc(1);
        tokio::time::delay_for(Duration::from_millis(50)).await;
    }
    //progress_bar.finish();
    Ok(())
}
