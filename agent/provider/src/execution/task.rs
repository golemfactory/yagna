use super::exeunits_registry::ExeUnitInstance;


#[allow(dead_code)]
pub struct Task {
    exeunit: ExeUnitInstance,
    agreement_id: String,
    activity_id: String,
}

impl Task {
    pub fn new(exeunit: ExeUnitInstance, agreement_id: &str, activity_id: &str) -> Task {
        Task{ exeunit, agreement_id: agreement_id.to_string(), activity_id: activity_id.to_string() }
    }
}
