use super::model::{PaymentDescription, PaymentModel};
use super::pricing::LinearPricing;

use anyhow::Result;
use std::sync::Arc;

pub struct PaymentModelFactory;

impl PaymentModelFactory {
    pub fn create<'a>(commercials: &'a PaymentDescription<'a>) -> Result<Arc<dyn PaymentModel>> {
        Ok(Arc::new(LinearPricing::new(commercials)?))
    }
}
