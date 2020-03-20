use derive_more::Display;

use super::exeunit_instance::ExeUnitInstance;


#[derive(Display)]
#[display(fmt = "Task: agreement id [{}], activity id [{}], {}", agreement_id, activity_id, exeunit)]
pub struct Task {
    pub exeunit: ExeUnitInstance,
    pub agreement_id: String,
    pub activity_id: String,
}

impl Task {
    pub fn new(exeunit: ExeUnitInstance, agreement_id: &str, activity_id: &str) -> Task {
        Task {
            exeunit,
            agreement_id: agreement_id.to_string(),
            activity_id: activity_id.to_string(),
        }
    }
}

