use super::exeunit_instance::ExeUnitInstance;

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

impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Task")
            .field("agreement_id", &self.agreement_id)
            .field("activity_id", &self.activity_id)
            .finish()
    }
}
