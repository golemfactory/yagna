use super::linear_pricing::LinearPricing;
use super::model::{PaymentDescription, PaymentModel};

use anyhow::Result;
use std::sync::Arc;

pub struct PaymentModelFactory;

impl PaymentModelFactory {
    pub fn create(commercials: PaymentDescription) -> Result<Arc<dyn PaymentModel>> {
        Ok(Arc::new(LinearPricing::new(commercials)?))
    }
}
