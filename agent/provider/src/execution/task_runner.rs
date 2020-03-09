use actix::prelude::*;
use anyhow::{anyhow, bail, Error, Result};
use log_derive::{logfn, logfn_inputs};
use std::collections::HashSet;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use ya_client::activity::ActivityProviderApi;
use ya_model::activity::{
    activity_state::{State, StatePair},
    ActivityState, ProviderEvent,
};
use ya_utils_actix::actix_handler::ResultTypeGetter;
use ya_utils_actix::forward_actix_handler;

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

// =========================================== //
// TaskRunner declaration
// =========================================== //

pub struct TaskRunner {
    api: Arc<ActivityProviderApi>,
    registry: ExeUnitsRegistry,
    /// Spawned tasks.
    tasks: Vec<Task>,
    /// Agreements, that allow for CreateActivity event.
    active_agreements: HashSet<String>,
}

// outputing empty string for logfn macro purposes
impl std::fmt::Display for TaskRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "")
    }
}

impl TaskRunner {
    pub fn new(client: ActivityProviderApi) -> TaskRunner {
        TaskRunner {
            api: Arc::new(client),
            registry: ExeUnitsRegistry::new(),
            tasks: vec![],
            active_agreements: HashSet::new(),
        }
    }

    #[logfn_inputs(Info, fmt = "{}Initializing ExeUnit: {:?}")]
    #[logfn(ok = "INFO", err = "ERROR", fmt = "ExeUnits initialized: {:?}")]
    pub fn initialize_exeunits(&mut self, msg: InitializeExeUnits) -> Result<()> {
        self.registry
            .register_exeunits_from_file(&msg.file)
            .map(|_| ())
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

    #[logfn_inputs(Debug, fmt = "{}Processing {:?}")]
    #[logfn(ok = "INFO", err = "ERROR", fmt = "Activity created: {:?}")]
    fn on_create_activity(&mut self, msg: CreateActivity) -> Result<()> {
        if !self.active_agreements.contains(&msg.agreement_id) {
            bail!("Can't create activity for not my agreement [{:?}].", msg);
        }

        // TODO: Get ExeUnit name from agreement.
        let exeunit_name = "wasmtime";
        match self.create_task(exeunit_name, &msg.activity_id, &msg.agreement_id) {
            Ok(task) => {
                self.tasks.push(task);
                Ok(())
            }
            Err(error) => bail!("Error creating activity: {:?}: {}", msg, error),
        }
    }

    #[logfn_inputs(Debug, fmt = "{}Processing {:?}")]
    #[logfn(ok = "INFO", err = "ERROR", fmt = "Activity destroyed: {:?}")]
    fn on_destroy_activity(&mut self, msg: DestroyActivity) -> Result<()> {
        match self.tasks.iter().position(|task| {
            task.agreement_id == msg.agreement_id && task.activity_id == msg.activity_id
        }) {
            None => bail!("Can't destroy not existing activity [{:?}]", msg),
            Some(task_position) => {
                // Remove task from list and destroy everything related with it.
                let task = self.tasks.swap_remove(task_position);
                TaskRunner::destroy_task(task);

                // TODO: remove this
                let client = self.api.clone();
                Arbiter::spawn(async move {
                    log::debug!("changing activity state to: Terminated");
                    if let Err(e) = client
                        .set_activity_state(
                            &msg.activity_id,
                            &ActivityState::from(StatePair::from(State::Terminated)),
                        )
                        .await
                    {
                        log::error!("Setting state for activity [{:?}], error: {}", msg, e);
                    }
                })
            }
        }
        Ok(())
    }

    #[logfn_inputs(Info, fmt = "{}Got {:?}")]
    pub fn on_agreement_approved(&mut self, msg: AgreementApproved) -> Result<()> {
        // Agreement waits for first create activity.
        // FIXME: clean-up agreements upon TTL or maybe payments
        self.active_agreements.insert(msg.agreement_id);
        Ok(())
    }

    #[logfn_inputs(Info, fmt = "{}Creating task: {}, act id: {}, agrmnt id: {}")]
    #[logfn(Debug, fmt = "Task created: {:?}")]
    fn create_task(
        &self,
        exeunit_name: &str,
        activity_id: &str,
        agreement_id: &str,
    ) -> Result<Task> {
        let exeunit_working_dir = env::current_dir()?;
        let exeunit_instance = self
            .registry
            .spawn_exeunit(exeunit_name, &exeunit_working_dir)
            .map_err(|error| {
                anyhow!(
                    "Spawning ExeUnit failed for agreement [{}] with error: {}",
                    agreement_id,
                    error
                )
            })?;

        Ok(Task::new(exeunit_instance, agreement_id, activity_id))
    }

    #[logfn_inputs(Info, fmt = "Destroying task: {:?}")]
    #[logfn(Debug, fmt = "Task destroyed: {:?}")]
    fn destroy_task(mut task: Task) {
        // Here we could cleanup resources, directories and everything.
        task.exeunit.kill();
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
