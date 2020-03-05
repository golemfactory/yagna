use super::model::{PaymentModel, PaymentDescription};

use anyhow::Result;
use std::sync::Arc;



pub struct PaymentModelFactory;

impl PaymentModelFactory {
    pub fn create(commercials: PaymentDescription) -> Result<Arc<Box<dyn PaymentModel>>> {
        unimplemented!()
    }
}
