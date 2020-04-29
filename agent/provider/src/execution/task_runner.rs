use actix::prelude::*;
use anyhow::{anyhow, bail, Error, Result};
use derive_more::Display;
use futures::future::join_all;
use humantime;
use log_derive::{logfn, logfn_inputs};
use std::collections::HashMap;
use std::fs::{create_dir_all, File};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use structopt::StructOpt;

use ya_agreement_utils::ParsedAgreement;
use ya_client::activity::ActivityProviderApi;
use ya_core_model::activity;
use ya_model::activity::{ActivityState, ProviderEvent, State, StatePair};
use ya_utils_actix::actix_handler::ResultTypeGetter;
use ya_utils_actix::actix_signal::{SignalSlot, Subscribe};
use ya_utils_actix::forward_actix_handler;
use ya_utils_path::SecurePath;
use ya_utils_process::ExeUnitExitStatus;

use super::exeunits_registry::{ExeUnitDesc, ExeUnitsRegistry};
use super::task::Task;
use crate::market::provider_market::AgreementApproved;
use crate::task_manager::{AgreementBroken, AgreementClosed};

// =========================================== //
// Public exposed messages
// =========================================== //

/// Collects activity events and processes them.
/// This event should be sent periodically.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct UpdateActivity;

/// Loads ExeUnits descriptors from file.
#[derive(Message, Debug)]
#[rtype(result = "Result<()>")]
pub struct InitializeExeUnits {
    pub file: PathBuf,
}

/// Finds ExeUnit in registry and returns it's descriptor.
#[derive(Message)]
#[rtype(result = "Result<ExeUnitDesc>")]
pub struct GetExeUnit {
    pub name: String,
}

// =========================================== //
// Public signals sent by TaskRunner
// =========================================== //

/// Signal emitted when TaskRunner finished processing
/// of CreateActivity event. That means, that ExeUnit is already created.
#[derive(Message, Clone)]
#[rtype(result = "Result<()>")]
pub struct ActivityCreated {
    pub agreement_id: String,
    pub activity_id: String,
}

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
#[derive(Message, Debug)]
#[rtype(result = "Result<()>")]
struct CreateActivity {
    pub activity_id: String,
    pub agreement_id: String,
}

/// Called when we got destroyActivity event.
#[derive(Message, Debug)]
#[rtype(result = "Result<()>")]
struct DestroyActivity {
    pub activity_id: String,
    pub agreement_id: String,
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
    active_agreements: HashMap<String, ParsedAgreement>,

    /// External actors can listen on these signals.
    pub activity_created: SignalSlot<ActivityCreated>,
    pub activity_destroyed: SignalSlot<ActivityDestroyed>,

    config: Arc<TaskRunnerConfig>,

    tasks_dir: PathBuf,
    cache_dir: PathBuf,
}

impl TaskRunner {
    pub fn new(client: ActivityProviderApi, config: TaskRunnerConfig) -> Result<TaskRunner> {
        let current_dir = std::env::current_dir()?;
        let tasks_dir = current_dir.join("exe-unit").join("work");
        let cache_dir = current_dir.join("exe-unit").join("cache");

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
        current_dir.to_str().ok_or(anyhow!(
            "Current dir [{}] contains invalid characters.",
            current_dir.display()
        ))?;

        Ok(TaskRunner {
            api: Arc::new(client),
            registry: ExeUnitsRegistry::new(),
            tasks: vec![],
            active_agreements: HashMap::new(),
            activity_created: SignalSlot::<ActivityCreated>::new(),
            activity_destroyed: SignalSlot::<ActivityDestroyed>::new(),
            config: Arc::new(config),
            tasks_dir,
            cache_dir,
        })
    }

    #[logfn(ok = "INFO", err = "ERROR", fmt = "ExeUnits initialized: {:?}")]
    pub fn initialize_exeunits(
        &mut self,
        msg: InitializeExeUnits,
        _ctx: &mut Context<Self>,
    ) -> Result<()> {
        log::info!("Initializing ExeUnits from file: [{}]", msg.file.display());

        self.registry.register_exeunits_from_file(&msg.file)?;
        Ok(self.registry.validate()?)
    }

    pub fn get_exeunit(
        &mut self,
        msg: GetExeUnit,
        _ctx: &mut Context<Self>,
    ) -> Result<ExeUnitDesc> {
        self.registry.find_exeunit(&msg.name)
    }

    pub async fn collect_events(
        client: Arc<ActivityProviderApi>,
        addr: Addr<TaskRunner>,
    ) -> Result<()> {
        match TaskRunner::query_events(client).await {
            Err(error) => log::error!("Can't query activity events. Error: {:?}", error),
            Ok(activity_events) => {
                TaskRunner::dispatch_events(&activity_events, addr).await;
            }
        }

        Ok(())
    }

    // =========================================== //
    // TaskRunner internals - events dispatching
    // =========================================== //

    async fn dispatch_events(events: &Vec<ProviderEvent>, notify: Addr<TaskRunner>) {
        if events.len() == 0 {
            return;
        };

        log::info!("Collected {} activity events. Processing...", events.len());

        // FIXME: Create activity arrives together with destroy, and destroy is being processed first
        let futures = events
            .into_iter()
            .zip(vec![notify.clone()].into_iter().cycle())
            .map(|(event, notify)| async move {
                let _ = match event {
                    ProviderEvent::CreateActivity {
                        activity_id,
                        agreement_id,
                    } => {
                        notify
                            .send(CreateActivity::new(activity_id, agreement_id))
                            .await?
                    }
                    ProviderEvent::DestroyActivity {
                        activity_id,
                        agreement_id,
                    } => {
                        notify
                            .send(DestroyActivity::new(activity_id, agreement_id))
                            .await?
                    }
                }
                .map_err(|error| log::warn!("{}", error));
                Result::<(), anyhow::Error>::Ok(())
            })
            .collect::<Vec<_>>();

        let _ = join_all(futures).await;
    }

    async fn query_events(client: Arc<ActivityProviderApi>) -> Result<Vec<ProviderEvent>> {
        Ok(client.get_activity_events(Some(3.), None).await?)
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

        let task = match self.create_task(&exeunit_name, &msg.activity_id, &msg.agreement_id) {
            Ok(task) => task,
            Err(error) => bail!("Error creating activity: {:?}: {}", msg, error),
        };

        let process = task.exeunit.get_process_handle();
        self.tasks.push(task);

        let _ = self.activity_created.send_signal(ActivityCreated {
            agreement_id: msg.agreement_id.clone(),
            activity_id: msg.activity_id.clone(),
        });

        // We need to discover that ExeUnit process finished.
        // We can't be sure that Requestor will send DestroyActivity.
        let self_addr = ctx.address();
        let activity_id = msg.activity_id.clone();
        let agreement_id = msg.agreement_id.clone();

        Arbiter::spawn(async move {
            let status = process.wait_until_finished().await;
            let msg = ExeUnitProcessFinished {
                activity_id,
                agreement_id,
                status,
            };

            self_addr.do_send(msg);
        });

        Ok(())
    }

    fn on_destroy_activity(
        &mut self,
        msg: DestroyActivity,
        _ctx: &mut Context<Self>,
    ) -> Result<()> {
        log::info!("Destroying activity [{}].", msg.activity_id);

        let task_position = match self.tasks.iter().position(|task| {
            task.agreement_id == msg.agreement_id && task.activity_id == msg.activity_id
        }) {
            None => bail!("Can't destroy not existing activity [{:?}]", msg),
            Some(task_position) => task_position,
        };

        // Remove task from list and destroy everything related with it.
        let task = self.tasks.swap_remove(task_position);
        let termination_timeout = self.config.process_termination_timeout;
        let state_retry_interval = self.config.exeunit_state_retry_interval;
        let api = self.api.clone();

        Arbiter::spawn(async move {
            if let Err(error) = task.exeunit.terminate(termination_timeout).await {
                log::warn!(
                    "Could not terminate ExeUnit for activity: [{}]. Error: {}. Killing instead.",
                    msg.activity_id,
                    error
                );
                task.exeunit.kill();

                // It was brutal termination and ExeUnit probably didn't set state.
                // We must do it instead of him. Repeat until it will succeed.
                set_activity_terminated(api, &task.activity_id, state_retry_interval).await;
            }

            log::info!("ExeUnit for activity terminated: [{}].", msg.activity_id);
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
            msg.agreement_id
        );

        let destroy_msg = ActivityDestroyed {
            agreement_id: msg.agreement_id.to_string(),
            activity_id: msg.activity_id.clone(),
        };
        let _ = self.activity_destroyed.send_signal(destroy_msg.clone());
        Ok(())
    }

    #[logfn_inputs(Debug, fmt = "{}Got {:?} {:?}")]
    pub fn on_agreement_approved(
        &mut self,
        msg: AgreementApproved,
        _ctx: &mut Context<Self>,
    ) -> Result<()> {
        // Agreement waits for first create activity event.
        let agreement_id = msg.agreement.agreement_id.clone();
        self.active_agreements.insert(agreement_id, msg.agreement);
        Ok(())
    }

    pub fn on_agreement_broken(
        &mut self,
        msg: AgreementBroken,
        ctx: &mut Context<Self>,
    ) -> Result<()> {
        self.active_agreements.remove(&msg.agreement_id);
        self.remove_remaining_tasks(&msg.agreement_id, ctx.address().clone());
        Ok(())
    }

    fn on_agreement_closed(&mut self, msg: AgreementClosed, ctx: &mut Context<Self>) -> Result<()> {
        self.active_agreements.remove(&msg.agreement_id);
        // All activities should be destroyed by now, so it is only sanity call.
        self.remove_remaining_tasks(&msg.agreement_id, ctx.address().clone());
        Ok(())
    }

    fn remove_remaining_tasks(&mut self, agreement_id: &str, addr: Addr<Self>) {
        self.tasks
            .iter()
            .filter(|task| task.agreement_id == agreement_id)
            .for_each(|task| {
                log::warn!(
                    "Activity [{}] will be destroyed, because of terminated agreement [{}].",
                    task.activity_id,
                    agreement_id,
                );
                addr.do_send(DestroyActivity::new(&task.activity_id, &agreement_id));
            });
    }

    #[logfn(Debug, fmt = "Task created: {}")]
    fn create_task(
        &self,
        exeunit_name: &str,
        activity_id: &str,
        agreement_id: &str,
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

        let mut args = vec![];
        args.extend(["-c", self.cache_dir.to_str().ok_or(anyhow!("None"))?].iter());
        args.extend(["-w", working_dir.to_str().ok_or(anyhow!("None"))?].iter());
        args.extend(["-a", agreement_path.to_str().ok_or(anyhow!("None"))?].iter());

        args.push("service-bus");
        args.push(activity_id);
        args.push(activity::local::BUS_ID);

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

    pub fn on_subscribe_activity_created(
        &mut self,
        msg: Subscribe<ActivityCreated>,
        _ctx: &mut Context<Self>,
    ) -> Result<()> {
        Ok(self.activity_created.on_subscribe(msg))
    }

    pub fn on_subscribe_activity_destroyed(
        &mut self,
        msg: Subscribe<ActivityDestroyed>,
        _ctx: &mut Context<Self>,
    ) -> Result<()> {
        Ok(self.activity_destroyed.on_subscribe(msg))
    }
}

fn exe_unit_name_from(agreement: &ParsedAgreement) -> Result<String> {
    let runtime_key_str = "/offer/properties/golem/runtime/name";
    Ok(agreement.pointer_typed::<String>(runtime_key_str)?)
}

async fn set_activity_terminated(
    api: Arc<ActivityProviderApi>,
    activity_id: &str,
    retry_interval: Duration,
) {
    let state = ActivityState::from(StatePair(State::Terminated, None));
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

// =========================================== //
// Actix stuff
// =========================================== //

impl Actor for TaskRunner {
    type Context = Context<Self>;
}

forward_actix_handler!(TaskRunner, AgreementApproved, on_agreement_approved);
forward_actix_handler!(TaskRunner, InitializeExeUnits, initialize_exeunits);
forward_actix_handler!(TaskRunner, DestroyActivity, on_destroy_activity);
forward_actix_handler!(TaskRunner, ExeUnitProcessFinished, on_exeunit_exited);
forward_actix_handler!(TaskRunner, GetExeUnit, get_exeunit);
forward_actix_handler!(TaskRunner, AgreementBroken, on_agreement_broken);
forward_actix_handler!(TaskRunner, AgreementClosed, on_agreement_closed);

forward_actix_handler!(
    TaskRunner,
    Subscribe<ActivityCreated>,
    on_subscribe_activity_created
);
forward_actix_handler!(
    TaskRunner,
    Subscribe<ActivityDestroyed>,
    on_subscribe_activity_destroyed
);

impl Handler<UpdateActivity> for TaskRunner {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, _msg: UpdateActivity, ctx: &mut Context<Self>) -> Self::Result {
        let client = self.api.clone();
        let addr = ctx.address();

        ActorResponse::r#async(
            async move { TaskRunner::collect_events(client, addr).await }.into_actor(self),
        )
    }
}

impl Handler<CreateActivity> for TaskRunner {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: CreateActivity, ctx: &mut Context<Self>) -> Self::Result {
        let api = self.api.clone();
        let activity_id = msg.activity_id.clone();
        let state_retry_interval = self.config.exeunit_state_retry_interval.clone();

        let result = self.on_create_activity(msg, ctx);

        let on_error_future = async move {
            set_activity_terminated(api, &activity_id, state_retry_interval).await;
        }
        .into_actor(self);

        match result {
            Ok(_) => ActorResponse::reply(result),
            Err(error) => ActorResponse::r#async(on_error_future.map(|_, _, _| Err(error))),
        }
    }
}

// =========================================== //
// Messages creation
// =========================================== //

impl CreateActivity {
    pub fn new(activity_id: &str, agreement_id: &str) -> CreateActivity {
        CreateActivity {
            activity_id: activity_id.to_string(),
            agreement_id: agreement_id.to_string(),
        }
    }
}

impl DestroyActivity {
    pub fn new(activity_id: &str, agreement_id: &str) -> DestroyActivity {
        DestroyActivity {
            activity_id: activity_id.to_string(),
            agreement_id: agreement_id.to_string(),
        }
    }
}
