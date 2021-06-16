use actix::prelude::*;
use anyhow::{anyhow, bail, Error, Result};
use chrono::{DateTime, Utc};
use derive_more::Display;
use futures::future::join_all;
use futures::{FutureExt, TryFutureExt};
use humantime;
use log_derive::{logfn, logfn_inputs};
use std::collections::HashMap;
use std::fs::{create_dir_all, File};
use std::iter;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use structopt::StructOpt;

use ya_agreement_utils::{AgreementView, OfferTemplate};
use ya_client::activity::ActivityProviderApi;
use ya_client_model::activity::provider_event::ProviderEventType;
use ya_client_model::activity::{ActivityState, ProviderEvent, State};
use ya_core_model::activity;
use ya_std_utils::LogErr;
use ya_utils_actix::actix_handler::ResultTypeGetter;
use ya_utils_actix::actix_signal::{Signal, SignalSlot};
use ya_utils_actix::{actix_signal_handler, forward_actix_handler};
use ya_utils_path::SecurePath;
use ya_utils_process::ExeUnitExitStatus;

use super::exeunits_registry::{ExeUnitDesc, ExeUnitsRegistry};
use super::task::Task;
use crate::market::provider_market::NewAgreement;
use crate::market::Preset;
use crate::tasks::{AgreementBroken, AgreementClosed};

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
        let tasks_dir = data_dir.join("exe-unit").join("work");
        let cache_dir = data_dir.join("exe-unit").join("cache");

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
        data_dir.to_str().ok_or(anyhow!(
            "Current dir [{}] contains invalid characters.",
            data_dir.display()
        ))?;

        Ok(TaskRunner {
            api: Arc::new(client),
            registry,
            tasks: vec![],
            active_agreements: HashMap::new(),
            activity_created: SignalSlot::<CreateActivity>::new(),
            activity_destroyed: SignalSlot::<ActivityDestroyed>::new(),
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
        if events.len() == 0 {
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
    #[logfn(ok = "INFO", err = "ERROR", fmt = "Activity created: {:?}")]
    fn on_create_activity(&mut self, msg: CreateActivity, ctx: &mut Context<Self>) -> Result<()> {
        let agreement = match self.active_agreements.get(&msg.agreement_id) {
            None => bail!("Can't create activity for not my agreement [{:?}].", msg),
            Some(agreement) => agreement,
        };

        let exeunit_name = exe_unit_name_from(&agreement)?;

        let task = match self.create_task(
            &exeunit_name,
            &msg.activity_id,
            &msg.agreement_id,
            msg.requestor_pub_key.as_ref().map(|s| s.as_str()),
        ) {
            Ok(task) => task,
            Err(error) => bail!("Error creating activity: {:?}: {}", msg, error),
        };

        let process = task.exeunit.get_process_handle();
        self.tasks.push(task);

        // We need to discover that ExeUnit process finished.
        // We can't be sure that Requestor will send DestroyActivity.
        let myself = ctx.address();
        let activity_id = msg.activity_id.clone();
        let agreement_id = msg.agreement_id.clone();
        let state_retry_interval = self.config.exeunit_state_retry_interval;
        let api = self.api.clone();

        Arbiter::spawn(async move {
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
            activity_id: msg.activity_id.clone(),
        };
        let _ = self.activity_destroyed.send_signal(destroy_msg.clone());
        Ok(())
    }

    pub fn on_agreement_approved(
        &mut self,
        msg: NewAgreement,
        _ctx: &mut Context<Self>,
    ) -> Result<()> {
        log::debug!("[TaskRunner] Got new Agreement: {}", msg.agreement);

        // Agreement waits for first create activity event.
        let agreement_id = msg.agreement.agreement_id.clone();
        self.active_agreements.insert(agreement_id, msg.agreement);
        Ok(())
    }

    fn offer_template(&self, exeunit_name: &str) -> impl Future<Output = Result<String>> {
        let working_dir = self.tasks_dir.clone();
        let args = vec![String::from("offer-template")];
        self.registry
            .run_exeunit_with_output(exeunit_name, args, &working_dir)
            .map_err(|error| error.context(format!("ExeUnit offer-template command failed")))
    }

    #[logfn(Debug, fmt = "Task created: {}")]
    fn create_task(
        &self,
        exeunit_name: &str,
        activity_id: &str,
        agreement_id: &str,
        requestor_pub_key: Option<&str>,
    ) -> Result<Task> {
        let working_dir = self
            .tasks_dir
            .secure_join(agreement_id)
            .secure_join(activity_id);

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
            .ok_or(anyhow!("None"))? // Parent must exist, since we built this path.
            .join("agreement.json");

        self.save_agreement(&agreement_path, &agreement_id)?;

        let mut args = vec!["service-bus", activity_id, activity::local::BUS_ID];
        args.extend(["-c", self.cache_dir.to_str().ok_or(anyhow!("None"))?].iter());
        args.extend(["-w", working_dir.to_str().ok_or(anyhow!("None"))?].iter());
        args.extend(["-a", agreement_path.to_str().ok_or(anyhow!("None"))?].iter());

        if let Some(req_pub_key) = requestor_pub_key {
            args.extend(["--requestor-pub-key", req_pub_key.as_ref()].iter());
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
            .ok_or(anyhow!("Can't find agreement [{}].", agreement_id))?;

        let agreement_file = File::create(&agreement_path).map_err(|error| {
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
        tokio::time::delay_for(retry_interval).await;
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

impl Handler<GetOfferTemplates> for TaskRunner {
    type Result = ResponseFuture<Result<HashMap<String, OfferTemplate>>>;

    fn handle(&mut self, msg: GetOfferTemplates, _: &mut Context<Self>) -> Self::Result {
        let entries = msg
            .0
            .into_iter()
            .map(|p| (p.name, self.offer_template(&p.exeunit_name)))
            .collect::<Vec<_>>();

        async move {
            let mut result: HashMap<String, OfferTemplate> = HashMap::new();
            for (key, fut) in entries {
                log::info!("Reading offer template for {}", key);
                let string = fut.await?;
                let value = serde_json::from_str(string.as_str())?;
                log::info!("offer-template: {} = {:?}", key, value);
                result.insert(key, value);
            }
            Ok(result)
        }
        .boxed_local()
    }
}

impl Handler<UpdateActivity> for TaskRunner {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, _: UpdateActivity, ctx: &mut Context<Self>) -> Self::Result {
        let addr = ctx.address();
        let client = self.api.clone();

        let mut event_ts = self.event_ts.clone();
        let app_session_id = self.config.session_id.clone();
        let poll_timeout = Duration::from_secs(3);

        let fut = async move {
            let result = client
                .get_activity_events(
                    Some(event_ts.clone()),
                    Some(app_session_id),
                    Some(poll_timeout),
                    None,
                )
                .await;

            match result {
                Ok(events) => {
                    events
                        .iter()
                        .max_by_key(|e| e.event_date)
                        .map(|e| event_ts = event_ts.max(e.event_date));
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
        let state_retry_interval = self.config.exeunit_state_retry_interval.clone();

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
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: CreateActivity, ctx: &mut Context<Self>) -> Self::Result {
        let api = self.api.clone();
        let activity_id = msg.activity_id.clone();
        let state_retry_interval = self.config.exeunit_state_retry_interval.clone();

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
    type Result = ActorResponse<Self, (), Error>;

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
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: AgreementClosed, ctx: &mut Context<Self>) -> Self::Result {
        let agreement_id = msg.agreement_id.to_string();
        let myself = ctx.address().clone();
        let activities = self.list_activities(&agreement_id);

        self.active_agreements.remove(&agreement_id);

        // All activities should be destroyed by now, so it is only sanity call.
        let remove_future = async move {
            remove_remaining_tasks(activities, agreement_id, myself).await;
            Ok(())
        };

        ActorResponse::r#async(remove_future.into_actor(self))
    }
}

impl Handler<AgreementBroken> for TaskRunner {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: AgreementBroken, ctx: &mut Context<Self>) -> Self::Result {
        let agreement_id = msg.agreement_id.to_string();
        let myself = ctx.address().clone();
        let activities = self.list_activities(&agreement_id);

        self.active_agreements.remove(&agreement_id);

        let remove_future = async move {
            remove_remaining_tasks(activities, agreement_id, myself).await;
            Ok(())
        };

        ActorResponse::r#async(remove_future.into_actor(self))
    }
}

impl Handler<Shutdown> for TaskRunner {
    type Result = ActorResponse<Self, (), Error>;

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
