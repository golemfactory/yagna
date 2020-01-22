use super::exeunits_registry::ExeUnitsRegistry;
use super::task::Task;
use crate::market::provider_market::AgreementSigned;
use crate::{gen_actix_handler_sync, gen_actix_handler_async};

use ya_client::activity::ProviderApiClient;
use ya_model::activity::ProviderEvent;

use actix::prelude::*;

use anyhow::{Error, Result};
use std::cell::RefCell;
use std::rc::Rc;
use log::{info, error};

// =========================================== //
// Public exposed messages
// =========================================== //

/// Collects activity events and processes them.
/// This event should be sent periodically.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct UpdateActivity;

// =========================================== //
// TaskRunner declaration
// =========================================== //

#[allow(dead_code)]
pub struct TaskRunner {
    api: ProviderApiClient,
    registry: ExeUnitsRegistry,
    tasks: Vec<Task>,
}

#[allow(dead_code)]
impl TaskRunner {
    pub fn new(client: ProviderApiClient) -> TaskRunner {
        TaskRunner {
            api: client,
            registry: ExeUnitsRegistry::new(),
            tasks: vec![],
        }
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

    async fn dispatch_events(&mut self, events: &Vec<ProviderEvent>) {
        info!("Collected {} activity events. Processing...", events.len());

        for event in events.iter(){
            match event {
                ProviderEvent::CreateActivity {activity_id, agreement_id} =>
                    self.on_create_activity(activity_id, agreement_id),
                ProviderEvent::DestroyActivity {activity_id, agreement_id} =>
                    self.on_destroy_activity(activity_id, agreement_id)
            }
        }
    }

    async fn query_events(&self) -> Result<Vec<ProviderEvent>> {
        Ok(self.api.get_activity_events(Some(3)).await?)
    }

    // =========================================== //
    // TaskRunner internals - activity reactions
    // =========================================== //

    pub fn on_create_activity(&mut self, activity_id: &str, agreement_id: &str) {
        unimplemented!();
    }

    pub fn on_destroy_activity(&mut self, activity_id: &str, agreement_id: &str) {
        unimplemented!();
    }

    pub fn on_signed_agreement(&mut self, msg: AgreementSigned) -> Result<()> {
        info!("TaskRunner got signed agreement for processing.");

        //unimplemented!();
        Ok(())
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
        TaskRunnerActor{runner: Rc::new(RefCell::new(TaskRunner::new(client)))}
    }
}

gen_actix_handler_sync!(TaskRunnerActor, AgreementSigned, on_signed_agreement, runner);
gen_actix_handler_async!(TaskRunnerActor, UpdateActivity, collect_events, runner);