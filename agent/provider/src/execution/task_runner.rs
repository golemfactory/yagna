use super::exeunits_registry::ExeUnitsRegistry;
use super::task::Task;
use crate::market::provider_market::AgreementSigned;
use crate::{gen_actix_handler_sync};
use crate::utils::actix_handler::ResultTypeGetter;

use ya_client::activity::ProviderApiClient;
use ya_model::activity::ProviderEvent;

use actix::prelude::*;

use anyhow::{Error, Result};
use log::{error, info, warn};
use std::collections::HashSet;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;


// =========================================== //
// Public exposed messages
// =========================================== //

/// Collects activity events and processes them.
/// This event should be sent periodically.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct UpdateActivity;

/// Loads ExeUnits descriptors from file.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct InitializeExeUnits {
    pub file: PathBuf,
}

// =========================================== //
// Internal messages
// =========================================== //

/// Called when we got createActivity event.
#[derive(Message)]
#[rtype(result = "Result<()>")]
struct CreateActivity {
    pub activity_id: String,
    pub agreement_id: String,
}

/// Called when we got destroyActivity event.
#[derive(Message)]
#[rtype(result = "Result<()>")]
struct DestroyActivity {
    pub activity_id: String,
    pub agreement_id: String,
}

// =========================================== //
// TaskRunner declaration
// =========================================== //

pub struct TaskRunner {
    api: Arc<ProviderApiClient>,
    registry: ExeUnitsRegistry,
    /// Spawned tasks.
    tasks: Vec<Task>,
    /// Agreements, that wait for CreateActivity event.
    waiting_agreements: HashSet<String>,
}

impl TaskRunner {
    pub fn new(client: ProviderApiClient) -> TaskRunner {
        TaskRunner {
            api: Arc::new(client),
            registry: ExeUnitsRegistry::new(),
            tasks: vec![],
            waiting_agreements: HashSet::new(),
        }
    }

    pub fn initialize_exeunits(&mut self, msg: InitializeExeUnits) -> Result<()> {
        if let Err(error) = self.registry.register_exeunits_from_file(&msg.file) {
            error!("Can't initialize ExeUnits. {}", error);
            return Err(error);
        }
        info!("ExeUnits initialized from file {}.", msg.file.display());
        Ok(())
    }

    pub async fn collect_events(client: Arc<ProviderApiClient>, notify: Addr<TaskRunner>) -> Result<()> {
        let result = TaskRunner::query_events(client).await;
        match result {
            Err(error) => error!("Can't query activity events. Error: {}", error),
            Ok(activity_events) => {
                TaskRunner::dispatch_events(&activity_events, notify).await;
            }
        }

        Ok(())
    }

    // =========================================== //
    // TaskRunner internals - events dispatching
    // =========================================== //

    async fn dispatch_events(events: &Vec<ProviderEvent>, notify: Addr<TaskRunner>) {
        info!("Collected {} activity events. Processing...", events.len());

        for event in events.iter() {
            match event {
                ProviderEvent::CreateActivity {
                    activity_id,
                    agreement_id,
                } => {
                    if let Err(error) = notify.send(CreateActivity::new(&activity_id, &agreement_id)).await {
                        warn!("{}", error);
                    }
                }
                ProviderEvent::DestroyActivity {
                    activity_id,
                    agreement_id,
                } => {
                    if let Err(error) = notify.send(DestroyActivity::new(activity_id, agreement_id)).await {
                        warn!("{}", error);
                    }
                }
            }
        }
    }

    async fn query_events(client: Arc<ProviderApiClient>) -> Result<Vec<ProviderEvent>> {
        Ok(client.get_activity_events(Some(3)).await?)
    }

    // =========================================== //
    // TaskRunner internals - activity reactions
    // =========================================== //

    fn on_create_activity(&mut self, msg: CreateActivity) -> Result<()> {
        let activity_id = &msg.activity_id;
        let agreement_id = &msg.agreement_id;

        if !self.waiting_agreements.contains(agreement_id) {
            let msg = format!(
                "Trying to create activity [{}] for not my agreement [{}].",
                activity_id, agreement_id
            );
            return Err(Error::msg(msg));
        }

        if self.find_activity(activity_id, agreement_id).is_some() {
            let msg = format!(
                "Trying to create activity [{}] for the same agreeement [{}].",
                activity_id, agreement_id
            );
            return Err(Error::msg(msg));
        }

        // TODO: Get ExeUnit name from agreement.
        let exeunit_name = "dummy";
        match self.create_task(exeunit_name, activity_id, agreement_id) {
            Ok(task) => {
                self.waiting_agreements.remove(agreement_id);
                self.tasks.push(task);

                info!(
                    "Created activity [{}] for agreement [{}]. Spawned [{}] exeunit.",
                    activity_id, agreement_id, exeunit_name
                );
                Ok(())
            }
            Err(error) => return Err(Error::msg(format!("Can't create activity. {}", error))),
        }
    }

    fn on_destroy_activity(&mut self, msg: DestroyActivity) -> Result<()> {
        let activity_id: &str = &msg.activity_id;
        let agreement_id: &str = &msg.agreement_id;

        match self
            .tasks
            .iter()
            .position(|task| task.agreement_id == agreement_id && task.activity_id == activity_id)
        {
            None => {
                warn!(
                    "Trying to destroy not existing activity [{}]. Agreement [{}].",
                    activity_id, agreement_id
                );
                return Ok(());
            }
            Some(task_position) => {
                // Remove task from list and destroy everything related with it.
                let task = self.tasks.swap_remove(task_position);
                TaskRunner::destroy_task(task);
            }
        }
        Ok(())
    }

    pub fn on_signed_agreement(&mut self, msg: AgreementSigned) -> Result<()> {
        info!(
            "TaskRunner got signed agreement [{}] for processing.",
            &msg.agreement_id
        );

        // Agreement waits for create activity.
        self.waiting_agreements.insert(msg.agreement_id);
        Ok(())
    }

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
                Error::msg(format!(
                    "Spawning ExeUnit failed for agreement [{}] with error: {}",
                    agreement_id, error
                ))
            })?;

        Ok(Task::new(exeunit_instance, agreement_id, activity_id))
    }

    fn find_activity(&self, activity_id: &str, agreement_id: &str) -> Option<&Task> {
        self.tasks
            .iter()
            .find(|task| task.agreement_id == agreement_id && task.activity_id == activity_id)
    }

    fn destroy_task(mut task: Task) {
        info!(
            "Destroying task related to agreement [{}] and activity {}.",
            &task.agreement_id, &task.activity_id
        );

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

gen_actix_handler_sync!(
    TaskRunner,
    AgreementSigned,
    on_signed_agreement
);
gen_actix_handler_sync!(
    TaskRunner,
    InitializeExeUnits,
    initialize_exeunits
);
gen_actix_handler_sync!(
    TaskRunner,
    CreateActivity,
    on_create_activity
);
gen_actix_handler_sync!(
    TaskRunner,
    DestroyActivity,
    on_destroy_activity
);


impl Handler<UpdateActivity> for TaskRunner {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, _msg: UpdateActivity, ctx: &mut Context<Self>) -> Self::Result {
        let client = self.api.clone();
        let addr = ctx.address();

        ActorResponse::r#async(
            async move { TaskRunner::collect_events(client, addr).await }
            .into_actor(self),
        )
    }
}

// =========================================== //
// Messages creation
// =========================================== //

impl CreateActivity {
    pub fn new(activity_id: &str, agreement_id: &str) -> CreateActivity {
        CreateActivity{activity_id: activity_id.to_string(), agreement_id: agreement_id.to_string()}
    }
}

impl DestroyActivity {
    pub fn new(activity_id: &str, agreement_id: &str) -> DestroyActivity {
        DestroyActivity{activity_id: activity_id.to_string(), agreement_id: agreement_id.to_string()}
    }
}
