mod api;
mod fs;
mod model;
mod parser;
mod startup;

pub use api::{
    have_consent_cached, run_consent_command, set_consent, set_consent_path_in_yagna_dir,
};
pub use model::{ConsentCommand, ConsentEntry, ConsentScope};
pub use startup::consent_check_before_startup;

use ya_service_api_interfaces::*;

pub struct ConsentService;

impl Service for ConsentService {
    type Cli = ConsentCommand;
}
