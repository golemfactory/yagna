use actix::prelude::*;
use anyhow::{anyhow, bail, Error, Result};
use chrono::{DateTime, Utc};
use derive_more::Display;
use futures::future::{join_all, select, Either};
use futures::{Future, FutureExt, TryFutureExt};
use humantime;
use log_derive::{logfn, logfn_inputs};
use std::collections::HashMap;
use std::fs::{create_dir_all, File};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use std::{fs, iter};
use structopt::StructOpt;

use ya_agreement_utils::{AgreementView, OfferTemplate};
use ya_client::activity::ActivityProviderApi;
use ya_client::model::activity::provider_event::ProviderEventType;
use ya_client::model::activity::{ActivityState, ProviderEvent, State, StatePair};
use ya_std_utils::LogErr;
use ya_utils_actix::actix_handler::ResultTypeGetter;
use ya_utils_actix::actix_signal::{Signal, SignalSlot};
use ya_utils_actix::{actix_signal_handler, forward_actix_handler};
use ya_utils_path::SecurePath;
use ya_utils_process::ExeUnitExitStatus;

use super::registry::{ExeUnitDesc, ExeUnitsRegistry};
use super::task::Task;
use crate::market::provider_market::NewAgreement;
use crate::market::Preset;
use crate::tasks::{AgreementBroken, AgreementClosed};

const EXE_UNIT_DIR: &str = "exe-unit";
const WORK_DIR: &str = "work";
const CACHE_DIR: &str = "cache";

// =========================================== //
// Public exposed messages
// =========================================== //

/// Collects activity events and processes them.
/// This event should be sent periodically.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct UpdateActivity;

/// Finds ExeUnit in registry and returns it's descriptor.
#[derive(Message)]
#[rtype(result = "Result<ExeUnitDesc>")]
pub struct GetExeUnit {
    pub name: String,
}

#[derive(Message)]
#[rtype(result = "Result<HashMap<String, OfferTemplate>>")]
pub struct GetOfferTemplates(pub Vec<Preset>);

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct Shutdown;

// =========================================== //
// Public signals sent by TaskRunner
// =========================================== //

/// Signal emitted when TaskRunner destroys activity and ExeUnit process exited.
/// It can happen in several situations:
/// - Requestor sends terminate command to ExeUnit
/// - Requestor sends DestroyActivity event
/// - Task is finished because of timeout
#[derive(Message, Clone, Debug)]
#[rtype(result = "Result<()>")]
pub struct ActivityDestroyed {
    pub agreement_id: String,
    pub activity_id: String,
}

// =========================================== //
// Internal messages
// =========================================== //

/// Called when we got createActivity event.
#[derive(Message, Clone, Debug)]
#[rtype(result = "Result<()>")]
pub struct CreateActivity {
    pub activity_id: String,
    pub agreement_id: String,
    pub requestor_pub_key: Option<String>,
}

/// Called when we got destroyActivity event.
#[derive(Message, Clone, Debug)]
#[rtype(result = "Result<()>")]
pub struct DestroyActivity {
    pub activity_id: String,
    pub agreement_id: String,
}

#[derive(Message, Clone, Debug)]
#[rtype(result = "()")]
pub struct TerminateActivity {
    pub activity_id: String,
    pub agreement_id: String,
    pub reason: String,
    pub message: String,
}

/// Called when process exited. There are 3 reasons for process to exit:
/// - We got DestroyActivity event and killed process.
/// - ExeUnit crashed.
#[derive(Message)]
#[rtype(result = "Result<()>")]
struct ExeUnitProcessFinished {
    pub activity_id: String,
    pub agreement_id: String,
    pub status: ExeUnitExitStatus,
}

// =========================================== //
// TaskRunner configuration
// =========================================== //

/// Configuration for TaskRunner actor.
#[derive(StructOpt, Clone, Debug)]
pub struct TaskRunnerConfig {
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "5s")]
    pub process_termination_timeout: Duration,
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "10s")]
    pub exeunit_state_retry_interval: Duration,
    /// Removes directory content after each `Activity` is destroyed.
    #[structopt(long, env)]
    pub auto_cleanup_activity: bool,
    /// Removes directory content after `Agreement` is terminated.
    /// Use this option to save disk space. Shouldn't be used when debugging.
    #[structopt(long, env)]
    pub auto_cleanup_agreement: bool,
    #[structopt(skip = "you-forgot-to-set-session-id")]
    pub session_id: String,
}

// =========================================== //
// TaskRunner declaration
// =========================================== //

// Outputing empty string for logfn macro purposes
#[derive(Display)]
#[display(fmt = "")]
pub struct TaskRunner {
    api: Arc<ActivityProviderApi>,
    registry: ExeUnitsRegistry,
    /// Spawned tasks.
    tasks: Vec<Task>,
    active_agreements: HashMap<String, AgreementView>,

    /// External actors can listen on these signals.
    pub activity_created: SignalSlot<CreateActivity>,
    pub activity_destroyed: SignalSlot<ActivityDestroyed>,

    config: Arc<TaskRunnerConfig>,

    event_ts: DateTime<Utc>,
    tasks_dir: PathBuf,
    cache_dir: PathBuf,
}

impl TaskRunner {
    pub fn new<P: AsRef<Path>>(
        client: ActivityProviderApi,
        config: TaskRunnerConfig,
        registry: ExeUnitsRegistry,
        data_dir: P,
    ) -> Result<TaskRunner> {
        let data_dir = data_dir.as_ref();
        let tasks_dir = exe_unit_work_dir(data_dir);
        let cache_dir = exe_unit_cache_dir(data_dir);

        log::debug!("TaskRunner config: {:?}", config);

        create_dir_all(&tasks_dir).map_err(|error| {
            anyhow!(
                "Can't create tasks directory [{}].. Error: {}",
                tasks_dir.display(),
                error
            )
        })?;

        create_dir_all(&cache_dir).map_err(|error| {
            anyhow!(
                "Can't create cache directory [{}]. Error: {}",
                cache_dir.display(),
                error
            )
        })?;

        // Try convert to str to check if won't fail. If not we can than
        // unwrap() all paths that we created relative to current_dir.
        data_dir.to_str().ok_or_else(|| {
            anyhow!(
                "Current dir [{}] contains invalid characters.",
                data_dir.display()
            )
        })?;

        Ok(TaskRunner {
            api: Arc::new(client),
            registry,
            tasks: vec![],
            active_agreements: HashMap::new(),
            activity_created: SignalSlot::<CreateActivity>::default(),
            activity_destroyed: SignalSlot::<ActivityDestroyed>::default(),
            config: Arc::new(config),
            event_ts: Utc::now(),
            tasks_dir,
            cache_dir,
        })
    }

    pub fn get_exeunit(
        &mut self,
        msg: GetExeUnit,
        _ctx: &mut Context<Self>,
    ) -> Result<ExeUnitDesc> {
        self.registry.find_exeunit(&msg.name)
    }

    // =========================================== //
    // TaskRunner internals - events dispatching
    // =========================================== //

    async fn dispatch_events(events: Vec<ProviderEvent>, myself: &Addr<TaskRunner>) {
        if events.is_empty() {
            return;
        };

        log::info!("Collected {} activity events. Processing...", events.len());

        // FIXME: Create activity arrives together with destroy, and destroy is being processed first
        let futures = events
            .into_iter()
            .zip(iter::repeat(myself))
            .map(|(event, myself)| async move {
                let _ = match event.event_type {
                    ProviderEventType::CreateActivity { requestor_pub_key } => {
                        myself
                            .send(Signal(CreateActivity {
                                activity_id: event.activity_id,
                                agreement_id: event.agreement_id,
                                requestor_pub_key,
                            }))
                            .await?
                    }
                    ProviderEventType::DestroyActivity {} => {
                        myself
                            .send(DestroyActivity {
                                activity_id: event.activity_id,
                                agreement_id: event.agreement_id,
                            })
                            .await?
                    }
                }
                .log_warn();
                Result::<(), anyhow::Error>::Ok(())
            })
            .collect::<Vec<_>>();

        let _ = join_all(futures).await;
    }

    // =========================================== //
    // TaskRunner internals - activity reactions
    // =========================================== //

    #[logfn_inputs(Debug, fmt = "{}Processing {:?} {:?}")]
    fn on_create_activity(&mut self, msg: CreateActivity, ctx: &mut Context<Self>) -> Result<()> {
        let agreement = match self.active_agreements.get(&msg.agreement_id) {
            None => bail!("Can't create activity for not my agreement [{:?}].", msg),
            Some(agreement) => agreement,
        };

        let exeunit_name = exe_unit_name_from(agreement)?;

        let task = match self.create_task(
            &exeunit_name,
            &msg.activity_id,
            &msg.agreement_id,
            msg.requestor_pub_key.as_deref(),
        ) {
            Ok(task) => task,
            Err(error) => bail!("Error creating activity: {:?}: {}", msg, error),
        };

        let process = task.exeunit.get_process_handle();
        self.tasks.push(task);

        // Log ExeUnit initialization message
        let activity_id = msg.activity_id.clone();
        let api = self.api.clone();
        let proc = process.clone();

        tokio::task::spawn_local(async move {
            let mut finished = Box::pin(proc.wait_until_finished());
            let mut monitor = StateMonitor::default();

            while let Either::Left((result, fut)) =
                select(Box::pin(api.get_activity_state(&activity_id)), finished).await
            {
                finished = fut;

                if let Ok(state) = result {
                    monitor.update(state.state);
                }
                monitor.sleep().await;
            }
        });

        // We need to discover that ExeUnit process finished.
        // We can't be sure that Requestor will send DestroyActivity.
        let myself = ctx.address();
        let activity_id = msg.activity_id.clone();
        let agreement_id = msg.agreement_id.clone();
        let state_retry_interval = self.config.exeunit_state_retry_interval;
        let api = self.api.clone();

        tokio::task::spawn_local(async move {
            let status = process.wait_until_finished().await;

            // If it was brutal termination than ExeUnit probably didn't set state.
            // We must do it instead of him. Repeat until it will succeed.
            match &status {
                ExeUnitExitStatus::Aborted(exit_status) => {
                    log::warn!(
                        "ExeUnit [{}] execution aborted. Setting activity [{}] state to Terminated",
                        exeunit_name,
                        activity_id
                    );

                    let reason = "execution aborted";
                    let msg = format!("exit code {:?}", exit_status.code());
                    set_activity_terminated(api, &activity_id, reason, msg, state_retry_interval)
                        .await;
                }
                ExeUnitExitStatus::Error(error) => {
                    log::warn!(
                        "ExeUnit [{}] execution failed: {}. Setting activity [{}] state to Terminated",
                        error, exeunit_name, activity_id
                    );

                    let reason = "execution error";
                    set_activity_terminated(api, &activity_id, reason, error, state_retry_interval)
                        .await;
                }
                _ => (),
            }

            let msg = ExeUnitProcessFinished {
                activity_id,
                agreement_id,
                status,
            };

            myself.do_send(msg);
        });

        Ok(())
    }

    fn on_exeunit_exited(
        &mut self,
        msg: ExeUnitProcessFinished,
        _ctx: &mut Context<Self>,
    ) -> Result<()> {
        log::info!(
            "ExeUnit process exited with status {}, agreement [{}], activity [{}].",
            msg.status,
            msg.agreement_id,
            msg.activity_id
        );

        let destroy_msg = ActivityDestroyed {
            agreement_id: msg.agreement_id.to_string(),
            activity_id: msg.activity_id,
        };
        let _ = self.activity_destroyed.send_signal(destroy_msg);
        Ok(())
    }

    pub fn on_agreement_approved(
        &mut self,
        msg: NewAgreement,
        _ctx: &mut Context<Self>,
    ) -> Result<()> {
        log::debug!("[TaskRunner] Got new Agreement: {}", msg.agreement);

        // Agreement waits for first create activity event.
        let agreement_id = msg.agreement.id.clone();
        self.active_agreements.insert(agreement_id, msg.agreement);
        Ok(())
    }

    fn offer_template(&self, exeunit_name: &str) -> impl Future<Output = Result<String>> {
        let working_dir = self.tasks_dir.clone();
        let args = vec![String::from("offer-template")];
        self.registry
            .run_exeunit_with_output(exeunit_name, args, &working_dir)
            .map_err(|error| error.context("ExeUnit offer-template command failed".to_string()))
    }

    fn exeunit_coeffs(&self, exeunit_name: &str) -> Result<Vec<String>> {
        Ok(match self.registry.find_exeunit(exeunit_name)?.config {
            Some(ref config) => (config.counters.iter())
                .filter_map(|(prop, cnt)| cnt.price.then(|| prop.clone()))
                .collect(),
            _ => Default::default(),
        })
    }

    fn agreement_dir(&self, agreement_id: &str) -> PathBuf {
        self.tasks_dir.secure_join(agreement_id)
    }

    #[logfn(Debug, fmt = "Task created: {}")]
    fn create_task(
        &self,
        exeunit_name: &str,
        activity_id: &str,
        agreement_id: &str,
        requestor_pub_key: Option<&str>,
    ) -> Result<Task> {
        let working_dir = self.agreement_dir(agreement_id).secure_join(activity_id);

        create_dir_all(&working_dir).map_err(|error| {
            anyhow!(
                "Can't create working directory [{}] for activity [{}]. Error: {}",
                working_dir.display(),
                activity_id,
                error
            )
        })?;

        let agreement_path = working_dir
            .parent()
            .ok_or_else(|| anyhow!("None"))? // Parent must exist, since we built this path.
            .join("agreement.json");

        self.save_agreement(&agreement_path, agreement_id)?;

        let mut args = vec![
            "service-bus",
            activity_id,
            ya_core_model::activity::local::BUS_ID,
        ];
        args.extend(
            [
                "-c",
                self.cache_dir.to_str().ok_or_else(|| anyhow!("None"))?,
            ]
            .iter(),
        );
        args.extend(["-w", working_dir.to_str().ok_or_else(|| anyhow!("None"))?].iter());
        args.extend(
            [
                "-a",
                agreement_path.to_str().ok_or_else(|| anyhow!("None"))?,
            ]
            .iter(),
        );

        if let Some(req_pub_key) = requestor_pub_key {
            args.extend(["--requestor-pub-key", req_pub_key].iter());
        }

        let args = args.iter().map(ToString::to_string).collect();

        log::info!(
            "Creating task: agreement [{}], activity [{}] in directory: [{}].",
            agreement_id,
            activity_id,
            working_dir.display()
        );

        let exeunit_instance = self
            .registry
            .spawn_exeunit(exeunit_name, args, &working_dir)
            .map_err(|error| {
                anyhow!(
                    "Spawning ExeUnit failed for agreement [{}] with error: {}",
                    agreement_id,
                    error
                )
            })?;

        Ok(Task::new(exeunit_instance, agreement_id, activity_id))
    }

    fn save_agreement(&self, agreement_path: &Path, agreement_id: &str) -> Result<()> {
        let agreement = self
            .active_agreements
            .get(agreement_id)
            .ok_or_else(|| anyhow!("Can't find agreement [{}].", agreement_id))?;

        let agreement_file = File::create(agreement_path).map_err(|error| {
            anyhow!(
                "Can't create agreement file [{}]. Error: {}",
                &agreement_path.display(),
                error
            )
        })?;

        serde_json::to_writer_pretty(&agreement_file, &agreement.json).map_err(|error| {
            anyhow!(
                "Failed to serialize agreement [{}]. Error: {}",
                agreement_id,
                error
            )
        })?;
        Ok(())
    }

    fn list_activities(&self, agreement_id: &str) -> Vec<String> {
        self.tasks
            .iter()
            .filter(|task| task.agreement_id == agreement_id)
            .map(|task| task.activity_id.to_string())
            .collect()
    }
}

pub fn exe_unit_work_dir<P: AsRef<Path>>(data_dir: P) -> PathBuf {
    let data_dir = data_dir.as_ref();
    data_dir.join(EXE_UNIT_DIR).join(WORK_DIR)
}

pub fn exe_unit_cache_dir<P: AsRef<Path>>(data_dir: P) -> PathBuf {
    let data_dir = data_dir.as_ref();
    data_dir.join(EXE_UNIT_DIR).join(CACHE_DIR)
}

fn exe_unit_name_from(agreement: &AgreementView) -> Result<String> {
    let runtime_key_str = "/offer/properties/golem/runtime/name";
    Ok(agreement.pointer_typed::<String>(runtime_key_str)?)
}

async fn set_activity_terminated(
    api: Arc<ActivityProviderApi>,
    activity_id: &str,
    reason: impl ToString,
    message: impl ToString,
    retry_interval: Duration,
) {
    let state = ActivityState {
        state: State::Terminated.into(),
        reason: Some(reason.to_string()),
        error_message: Some(message.to_string()),
    };

    // Potentially infinite loop. This is done intentionally.
    // Possible fail reasons:
    // - Lost network connection with yagna daemon ==> We should wait until yagna will be available.
    // - Wrong credentials => Probably we will face this problem on previous stages (create activity).
    // - Internal errors in yagna daemon => yagna needs restart/fix.
    while let Err(error) = api.set_activity_state(activity_id, &state).await {
        log::warn!(
            "Can't set terminated state for activity [{}]. Error: {}. Retry after: {:#?}",
            &activity_id,
            error,
            retry_interval
        );
        tokio::time::sleep(retry_interval).await;
    }
}

async fn remove_remaining_tasks(
    activities: Vec<String>,
    agreement_id: String,
    myself: Addr<TaskRunner>,
) {
    let destroy_futures = activities
        .iter()
        .zip(iter::repeat((myself, agreement_id)))
        .map(|(activity_id, (myself, agreement_id))| async move {
            log::warn!(
                "Activity [{}] will be destroyed, because of terminated agreement [{}].",
                activity_id,
                agreement_id,
            );
            myself
                .send(DestroyActivity {
                    activity_id: activity_id.clone(),
                    agreement_id,
                })
                .await
        })
        .collect::<Vec<_>>();
    let _ = join_all(destroy_futures).await;
}

// =========================================== //
// Actix stuff
// =========================================== //

impl Actor for TaskRunner {
    type Context = Context<Self>;
}

forward_actix_handler!(TaskRunner, NewAgreement, on_agreement_approved);
forward_actix_handler!(TaskRunner, ExeUnitProcessFinished, on_exeunit_exited);
forward_actix_handler!(TaskRunner, GetExeUnit, get_exeunit);
actix_signal_handler!(TaskRunner, CreateActivity, activity_created);
actix_signal_handler!(TaskRunner, ActivityDestroyed, activity_destroyed);

const PROPERTY_USAGE_VECTOR: &str = "golem.com.usage.vector";

impl Handler<GetOfferTemplates> for TaskRunner {
    type Result = ResponseFuture<Result<HashMap<String, OfferTemplate>>>;

    fn handle(&mut self, msg: GetOfferTemplates, _: &mut Context<Self>) -> Self::Result {
        let mut result: HashMap<String, OfferTemplate> = HashMap::with_capacity(msg.0.len());
        let entries = (msg.0.into_iter())
            .map(|preset| {
                let fut = self.offer_template(&preset.exeunit_name);
                let coeffs = self
                    .exeunit_coeffs(&preset.exeunit_name)
                    .map(|mut coll| {
                        coll.retain(|prop| preset.usage_coeffs.contains_key(prop));
                        coll
                    })
                    .unwrap_or_else(|_| Default::default());
                (preset, coeffs, fut)
            })
            .collect::<Vec<_>>();

        async move {
            for (preset, coeffs, fut) in entries {
                log::info!("Reading offer template for {}", preset.name);

                let output = fut.await?;
                let mut template: OfferTemplate = serde_json::from_str(output.as_str())?;

                match template
                    .property(PROPERTY_USAGE_VECTOR)
                    .ok_or_else(|| anyhow::anyhow!("offer template: missing usage vector"))?
                {
                    serde_json::Value::Array(vec) => {
                        let mut usage_vector = vec.clone();
                        usage_vector.extend(coeffs.into_iter().map(serde_json::Value::String));
                        template.set_property(
                            PROPERTY_USAGE_VECTOR,
                            serde_json::Value::Array(usage_vector),
                        );
                    }
                    _ => anyhow::bail!("offer template: invalid usage vector format"),
                }

                log::debug!("offer-template: {} = {:?}", preset.name, template);
                result.insert(preset.name, template);
            }
            Ok(result)
        }
        .boxed_local()
    }
}

impl Handler<UpdateActivity> for TaskRunner {
    type Result = ActorResponse<Self, Result<(), Error>>;

    fn handle(&mut self, _: UpdateActivity, ctx: &mut Context<Self>) -> Self::Result {
        let addr = ctx.address();
        let client = self.api.clone();

        let mut event_ts = self.event_ts;
        let app_session_id = self.config.session_id.clone();
        let poll_timeout = Duration::from_secs(3);

        let fut = async move {
            let result = client
                .get_activity_events(
                    Some(event_ts),
                    Some(app_session_id),
                    Some(poll_timeout),
                    None,
                )
                .await;

            match result {
                Ok(events) => {
                    if let Some(e) = events.iter().max_by_key(|e| e.event_date) {
                        event_ts = event_ts.max(e.event_date);
                    }
                    Self::dispatch_events(events, &addr).await;
                }
                Err(error) => log::error!("Can't query activity events: {:?}", error),
            };
            event_ts
        }
        .into_actor(self)
        .map(|event_ts, actor, _| {
            actor.event_ts = actor.event_ts.max(event_ts);
            Ok(())
        });

        ActorResponse::r#async(fut)
    }
}

impl Handler<TerminateActivity> for TaskRunner {
    type Result = ResponseFuture<()>;

    fn handle(&mut self, msg: TerminateActivity, _ctx: &mut Context<Self>) -> Self::Result {
        let api = self.api.clone();
        let state_retry_interval = self.config.exeunit_state_retry_interval;

        async move {
            set_activity_terminated(
                api,
                &msg.activity_id,
                msg.reason,
                msg.message,
                state_retry_interval,
            )
            .await
        }
        .boxed_local()
    }
}

impl Handler<CreateActivity> for TaskRunner {
    type Result = ActorResponse<Self, Result<(), Error>>;

    fn handle(&mut self, msg: CreateActivity, ctx: &mut Context<Self>) -> Self::Result {
        let api = self.api.clone();
        let activity_id = msg.activity_id.clone();
        let state_retry_interval = self.config.exeunit_state_retry_interval;

        let result = self.on_create_activity(msg, ctx);
        match result {
            Ok(_) => ActorResponse::reply(result),
            Err(error) => ActorResponse::r#async(
                async move {
                    set_activity_terminated(
                        api,
                        &activity_id,
                        "creation failed",
                        &error,
                        state_retry_interval,
                    )
                    .await;
                    Err(error)
                }
                .into_actor(self),
            ),
        }
    }
}

impl Handler<DestroyActivity> for TaskRunner {
    type Result = ActorResponse<Self, Result<(), Error>>;

    fn handle(&mut self, msg: DestroyActivity, _ctx: &mut Context<Self>) -> Self::Result {
        log::info!("Destroying activity [{}].", msg.activity_id);

        let task_position = match self.tasks.iter().position(|task| {
            task.agreement_id == msg.agreement_id && task.activity_id == msg.activity_id
        }) {
            None => {
                return ActorResponse::reply(Err(anyhow!(
                    "Can't destroy not existing activity [{}]",
                    msg.activity_id
                )))
            }
            Some(task_position) => task_position,
        };

        // Remove task from list and destroy everything related with it.
        let task = self.tasks.swap_remove(task_position);
        let termination_timeout = self.config.process_termination_timeout;

        let terminate = async move {
            if let Err(error) = task.exeunit.terminate(termination_timeout).await {
                log::warn!(
                    "Could not terminate ExeUnit for activity: [{}]. Error: {}. Killing instead.",
                    msg.activity_id,
                    error
                );
                task.exeunit.kill();
            }

            log::info!("ExeUnit for activity terminated: [{}].", msg.activity_id);
            Ok(())
        };

        ActorResponse::r#async(terminate.into_actor(self))
    }
}

impl Handler<AgreementClosed> for TaskRunner {
    type Result = ActorResponse<Self, Result<(), Error>>;

    fn handle(&mut self, msg: AgreementClosed, ctx: &mut Context<Self>) -> Self::Result {
        let agreement_id = msg.agreement_id;
        let myself = ctx.address();
        let activities = self.list_activities(&agreement_id);

        self.active_agreements.remove(&agreement_id);

        if self.config.auto_cleanup_agreement {
            fs::remove_dir_all(&self.agreement_dir(&agreement_id)).ok();
        }

        // All activities should be destroyed by now, so it is only sanity call.
        let remove_future = async move {
            remove_remaining_tasks(activities, agreement_id, myself).await;
            Ok(())
        };

        ActorResponse::r#async(remove_future.into_actor(self))
    }
}

impl Handler<AgreementBroken> for TaskRunner {
    type Result = ResponseFuture<Result<(), Error>>;

    fn handle(&mut self, msg: AgreementBroken, ctx: &mut Context<Self>) -> Self::Result {
        // We don't distinguish between `AgreementClosed` and `AgreementBroken`.
        let addr = ctx.address();
        async move {
            addr.send(AgreementClosed {
                agreement_id: msg.agreement_id,
                send_terminate: false,
            })
            .await?
        }
        .boxed_local()
    }
}

impl Handler<Shutdown> for TaskRunner {
    type Result = ActorResponse<Self, Result<(), Error>>;

    fn handle(&mut self, _: Shutdown, ctx: &mut Context<Self>) -> Self::Result {
        let ids = self
            .tasks
            .iter()
            .map(|t| (t.activity_id.clone(), t.agreement_id.clone()))
            .collect::<Vec<_>>();

        let addr = ctx.address();
        let fut = async move {
            for (activity_id, agreement_id) in ids {
                if let Err(e) = addr
                    .send(DestroyActivity {
                        activity_id,
                        agreement_id,
                    })
                    .await?
                {
                    log::error!("Unable to destroy activity: {}", e);
                }
            }
            Ok(())
        };

        ActorResponse::r#async(fut.into_actor(self))
    }
}

struct StateMonitor {
    state: StatePair,
    interval: Duration,
}

impl Default for StateMonitor {
    fn default() -> Self {
        StateMonitor {
            state: StatePair(State::New, None),
            interval: Self::INITIAL_INTERVAL,
        }
    }
}

impl StateMonitor {
    const INITIAL_INTERVAL: Duration = Duration::from_millis(750);
    const INTERVAL: Duration = Duration::from_secs(5);

    fn update(&mut self, state: StatePair) {
        match state {
            StatePair(State::Initialized, None)
            | StatePair(State::Deployed, _)
            | StatePair(State::Ready, _) => {
                if self.state.0 == State::New {
                    log::info!("ExeUnit initialized");
                    self.interval = Self::INTERVAL;
                }
            }
            StatePair(State::Unresponsive, _) => {
                if self.state.0 != State::Unresponsive {
                    log::warn!("ExeUnit is unresponsive");
                }
            }
            _ => {}
        }

        if self.state.0 == State::Unresponsive && state.0 != State::Unresponsive {
            log::warn!("ExeUnit is now responsive");
        }

        self.state = state;
    }

    fn sleep(&self) -> impl Future<Output = ()> {
        tokio::time::sleep(self.interval)
    }
}
