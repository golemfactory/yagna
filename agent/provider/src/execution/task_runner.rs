use super::exeunits_registry::ExeUnitsRegistry;
use super::task::Task;
use crate::market::provider_market::AgreementSigned;
use crate::{gen_actix_handler_async, gen_actix_handler_sync};

use ya_client::activity::ProviderApiClient;
use ya_model::activity::ProviderEvent;

use actix::prelude::*;

use anyhow::{Error, Result};
use log::{error, info, warn};
use std::cell::RefCell;
use std::collections::HashSet;
use std::env;
use std::path::PathBuf;
use std::rc::Rc;

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
// TaskRunner declaration
// =========================================== //

pub struct TaskRunner {
    api: ProviderApiClient,
    registry: ExeUnitsRegistry,
    /// Spawned tasks.
    tasks: Vec<Task>,
    /// Agreements, that wait for CreateActivity event.
    waiting_agreements: HashSet<String>,
}

impl TaskRunner {
    pub fn new(client: ProviderApiClient) -> TaskRunner {
        TaskRunner {
            api: client,
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

    pub async fn collect_events(&mut self, _msg: UpdateActivity) -> Result<()> {
        let result = self.query_events().await;
        match result {
            Err(error) => error!("Can't query activity events. Error: {}", error),
            Ok(activity_events) => {
                self.dispatch_events(&activity_events).await;
            }
        }

        Ok(())
    }

    // =========================================== //
    // TaskRunner internals - events dispatching
    // =========================================== //

    async fn dispatch_events(&mut self, events: &Vec<ProviderEvent>) {
        info!("Collected {} activity events. Processing...", events.len());

        for event in events.iter() {
            match event {
                ProviderEvent::CreateActivity {
                    activity_id,
                    agreement_id,
                } => {
                    if let Err(error) = self.on_create_activity(activity_id, agreement_id) {
                        warn!("{}", error);
                    }
                }
                ProviderEvent::DestroyActivity {
                    activity_id,
                    agreement_id,
                } => self.on_destroy_activity(activity_id, agreement_id),
            }
        }
    }

    async fn query_events(&self) -> Result<Vec<ProviderEvent>> {
        Ok(self.api.get_activity_events(Some(3)).await?)
    }

    // =========================================== //
    // TaskRunner internals - activity reactions
    // =========================================== //

    pub fn on_create_activity(&mut self, activity_id: &str, agreement_id: &str) -> Result<()> {
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

    pub fn on_destroy_activity(&mut self, activity_id: &str, agreement_id: &str) {
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
                return;
            }
            Some(task_position) => {
                // Remove task from list and destroy everything related with it.
                let task = self.tasks.swap_remove(task_position);
                TaskRunner::destroy_task(task);
            }
        }
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

pub struct TaskRunnerActor {
    runner: Rc<RefCell<TaskRunner>>,
}

impl Actor for TaskRunnerActor {
    type Context = Context<Self>;
}

impl TaskRunnerActor {
    pub fn new(client: ProviderApiClient) -> TaskRunnerActor {
        TaskRunnerActor {
            runner: Rc::new(RefCell::new(TaskRunner::new(client))),
        }
    }
}

gen_actix_handler_sync!(
    TaskRunnerActor,
    AgreementSigned,
    on_signed_agreement,
    runner
);
gen_actix_handler_async!(TaskRunnerActor, UpdateActivity, collect_events, runner);
gen_actix_handler_sync!(
    TaskRunnerActor,
    InitializeExeUnits,
    initialize_exeunits,
    runner
);

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use ya_client::activity::{provider::ProviderApiClient, ACTIVITY_API};
    use ya_client::web::WebClient;

    fn resources_directory() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-resources/")
    }

    fn create_runner() -> TaskRunner {
        let client = ProviderApiClient::new(
            WebClient::builder()
                .api_root(ACTIVITY_API)
                .build()
                .map(Arc::new)
                .unwrap(),
        );
        TaskRunner::new(client)
    }

    //    #[test]
    //    fn test_spawn_exeunit() {
    //        let mut runner = create_runner();
    //        let exeunits_descs_file = resources_directory().join("test-taskrunner-exeunits.json");
    //        let agreement_id = "blaaaa-agreement".to_string();
    //        let activity_id = "blaaaa-activity".to_string();
    //
    //        let msg = InitializeExeUnits {
    //            file: exeunits_descs_file,
    //        };
    //        runner.initialize_exeunits(msg).unwrap();
    //
    //        let msg = AgreementSigned {
    //            agreement_id: agreement_id.clone(),
    //        };
    //        runner.on_signed_agreement(msg).unwrap();
    //
    //        // Task should wait for create activity
    //        assert_eq!(runner.waiting_agreements.len(), 1);
    //
    //        runner
    //            .on_create_activity(&activity_id, &agreement_id)
    //            .unwrap();
    //
    //        // Task should be removed from waiting and inserted into spawned tasks.
    //        assert_eq!(runner.tasks.len(), 1);
    //        assert_eq!(runner.waiting_agreements.len(), 0);
    //    }
}
