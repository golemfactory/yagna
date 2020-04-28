use actix::prelude::*;
use anyhow::{anyhow, bail, Error, Result};
use derive_more::Display;
use log_derive::{logfn, logfn_inputs};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ya_client::activity::ActivityProviderApi;
use ya_core_model::activity;
use ya_model::activity::ProviderEvent;
use ya_model::market::Agreement;
use ya_utils_actix::actix_handler::ResultTypeGetter;
use ya_utils_actix::actix_signal::{SignalSlot, Subscribe};
use ya_utils_actix::forward_actix_handler;
use ya_utils_path::SecurePath;
use ya_utils_process::ExeUnitExitStatus;

use super::exeunits_registry::{ExeUnitDesc, ExeUnitsRegistry};
use super::task::Task;
use crate::market::provider_market::AgreementApproved;
use std::fs::{create_dir_all, File};

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
/// TODO: Load configuration from somewhere.
pub struct TaskRunnerConfig {
    pub process_termination_timeout: Duration,
}

impl Default for TaskRunnerConfig {
    fn default() -> Self {
        TaskRunnerConfig {
            process_termination_timeout: Duration::from_secs(5),
        }
    }
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
    /// Agreements, that wait for CreateActivity event.
    active_agreements: HashMap<String, Agreement>,

    /// External actors can listen on these signals.
    pub activity_created: SignalSlot<ActivityCreated>,
    pub activity_destroyed: SignalSlot<ActivityDestroyed>,

    config: Arc<TaskRunnerConfig>,

    tasks_dir: PathBuf,
    cache_dir: PathBuf,
}

impl TaskRunner {
    pub fn new(client: ActivityProviderApi, registry: ExeUnitsRegistry) -> Result<TaskRunner> {
        let current_dir = std::env::current_dir()?;
        let tasks_dir = current_dir.join("exe-unit").join("work");

        let cache_dir = current_dir.join("exe-unit").join("cache");

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
            registry,
            tasks: vec![],
            active_agreements: HashMap::new(),
            activity_created: SignalSlot::<ActivityCreated>::new(),
            activity_destroyed: SignalSlot::<ActivityDestroyed>::new(),
            config: Arc::new(TaskRunnerConfig::default()),
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
        for event in events.iter() {
            match event {
                ProviderEvent::CreateActivity {
                    activity_id,
                    agreement_id,
                } => {
                    if let Err(error) = notify
                        .send(CreateActivity::new(activity_id, agreement_id))
                        .await
                    {
                        log::warn!("{}", error);
                    }
                }
                ProviderEvent::DestroyActivity {
                    activity_id,
                    agreement_id,
                } => {
                    if let Err(error) = notify
                        .send(DestroyActivity::new(activity_id, agreement_id))
                        .await
                    {
                        log::warn!("{}", error);
                    }
                }
            }
        }
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

        let exeunit_name = task_package_from(&agreement)?;

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

        Arbiter::spawn(async move {
            if let Err(error) = task.exeunit.terminate(termination_timeout).await {
                log::warn!(
                    "Could not terminate ExeUnit for activity: [{}]. Error: {}",
                    msg.activity_id,
                    error
                );
                task.exeunit.kill();
            }

            log::info!("Activity destroyed: [{}].", msg.activity_id);
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
        // Agreement waits for first create activity.
        // FIXME: clean-up agreements upon TTL or maybe payments
        let agreement_id = msg.agreement.agreement_id.clone();
        self.active_agreements.insert(agreement_id, msg.agreement);
        Ok(())
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

        serde_json::to_writer_pretty(&agreement_file, &agreement).map_err(|error| {
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

fn task_package_from(agreement: &Agreement) -> Result<String> {
    let props = &agreement.offer.properties;
    let runtime_key_str = "golem.runtime.name";
    let runtime_name = props
        .as_object()
        .ok_or(anyhow!("Agreement properties has unexpected format."))?
        .iter()
        .find(|(key, _)| key == &runtime_key_str)
        .ok_or(anyhow!("Can't find key '{}'.", runtime_key_str))?
        .1
        .as_str()
        .ok_or(anyhow!("'{}' is not a string.", runtime_key_str))?;

    Ok(runtime_name.to_string())
}

// =========================================== //
// Actix stuff
// =========================================== //

impl Actor for TaskRunner {
    type Context = Context<Self>;
}

forward_actix_handler!(TaskRunner, AgreementApproved, on_agreement_approved);
forward_actix_handler!(TaskRunner, CreateActivity, on_create_activity);
forward_actix_handler!(TaskRunner, DestroyActivity, on_destroy_activity);
forward_actix_handler!(TaskRunner, ExeUnitProcessFinished, on_exeunit_exited);
forward_actix_handler!(TaskRunner, GetExeUnit, get_exeunit);

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
