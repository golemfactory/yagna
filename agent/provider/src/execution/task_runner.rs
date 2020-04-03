use actix::prelude::*;
use anyhow::{anyhow, bail, Error, Result};
use derive_more::Display;
use log_derive::{logfn, logfn_inputs};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ya_client::activity::ActivityProviderApi;
use ya_model::activity::ProviderEvent;
use ya_utils_actix::actix_handler::ResultTypeGetter;
use ya_utils_actix::actix_signal::{SignalSlot, Subscribe};
use ya_utils_actix::forward_actix_handler;
use ya_utils_process::ExeUnitExitStatus;

use super::exeunits_registry::ExeUnitsRegistry;
use super::task::Task;
use crate::market::provider_market::AgreementApproved;

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
    active_agreements: HashSet<String>,

    /// External actors can listen on these signals.
    pub activity_created: SignalSlot<ActivityCreated>,
    pub activity_destroyed: SignalSlot<ActivityDestroyed>,

    config: Arc<TaskRunnerConfig>,
}

impl TaskRunner {
    pub fn new(client: ActivityProviderApi) -> TaskRunner {
        TaskRunner {
            api: Arc::new(client),
            registry: ExeUnitsRegistry::new(),
            tasks: vec![],
            active_agreements: HashSet::new(),
            activity_created: SignalSlot::<ActivityCreated>::new(),
            activity_destroyed: SignalSlot::<ActivityDestroyed>::new(),
            config: Arc::new(TaskRunnerConfig::default()),
        }
    }

    #[logfn_inputs(Info, fmt = "{}Initializing ExeUnit: {:?} {:?}")]
    #[logfn(ok = "INFO", err = "ERROR", fmt = "ExeUnits initialized: {:?}")]
    pub fn initialize_exeunits(
        &mut self,
        msg: InitializeExeUnits,
        _ctx: &mut Context<Self>,
    ) -> Result<()> {
        self.registry.register_exeunits_from_file(&msg.file)
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
        Ok(client.get_activity_events(Some(3), None).await?)
    }

    // =========================================== //
    // TaskRunner internals - activity reactions
    // =========================================== //

    #[logfn_inputs(Debug, fmt = "{}Processing {:?} {:?}")]
    #[logfn(ok = "INFO", err = "ERROR", fmt = "Activity created: {:?}")]
    fn on_create_activity(&mut self, msg: CreateActivity, ctx: &mut Context<Self>) -> Result<()> {
        if !self.active_agreements.contains(&msg.agreement_id) {
            bail!("Can't create activity for not my agreement [{:?}].", msg);
        }

        // TODO: Get ExeUnit name from agreement.
        let exeunit_name = "wasmtime";
        let task = match self.create_task(exeunit_name, &msg.activity_id, &msg.agreement_id) {
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
        self.active_agreements.insert(msg.agreement.agreement_id);
        Ok(())
    }

    #[logfn_inputs(Info, fmt = "{}Creating task: {}, activity id: {}, agreement id: {}")]
    #[logfn(Debug, fmt = "Task created: {}")]
    fn create_task(
        &self,
        exeunit_name: &str,
        activity_id: &str,
        agreement_id: &str,
    ) -> Result<Task> {
        let exeunit_instance = self
            .registry
            .spawn_exeunit(exeunit_name, activity_id, agreement_id)
            .map_err(|error| {
                anyhow!(
                    "Spawning ExeUnit failed for agreement [{}] with error: {}",
                    agreement_id,
                    error
                )
            })?;

        Ok(Task::new(exeunit_instance, agreement_id, activity_id))
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

// =========================================== //
// Actix stuff
// =========================================== //

impl Actor for TaskRunner {
    type Context = Context<Self>;
}

forward_actix_handler!(TaskRunner, AgreementApproved, on_agreement_approved);
forward_actix_handler!(TaskRunner, InitializeExeUnits, initialize_exeunits);
forward_actix_handler!(TaskRunner, CreateActivity, on_create_activity);
forward_actix_handler!(TaskRunner, DestroyActivity, on_destroy_activity);
forward_actix_handler!(TaskRunner, ExeUnitProcessFinished, on_exeunit_exited);

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
