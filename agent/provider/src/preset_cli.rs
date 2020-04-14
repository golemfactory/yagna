use anyhow::Result;
use dialoguer::{Input, Select};

use crate::market::Preset;

pub struct PresetUpdater {
    preset: Preset,
    exeunits: Vec<String>,
    pricing_models: Vec<String>,
}

impl PresetUpdater {
    pub fn new(
        preset: Preset,
        exeunits: Vec<String>,
        pricing_models: Vec<String>,
    ) -> PresetUpdater {
        PresetUpdater {
            preset,
            exeunits,
            pricing_models,
        }
    }

    pub fn interact(mut self) -> Result<Preset> {
        self.preset.name = Input::<String>::new()
            .with_prompt("Preset name")
            .interact()?;

        let exeunit_idx = Select::new()
            .with_prompt("ExeUnit")
            .items(&self.exeunits[..])
            .default(0)
            .interact()?;
        self.preset.exeunit_name = self.exeunits[exeunit_idx].clone();

        let pricing_idx = Select::new()
            .with_prompt("Pricing model")
            .items(&self.pricing_models[..])
            .default(0)
            .interact()?;
        self.preset.pricing_model = self.pricing_models[pricing_idx].clone();

        let metrics = self.preset.list_readable_metrics();
        let usage_len = metrics.len() + 1;
        self.preset.usage_coeffs.resize(usage_len, 0.0);

        for (idx, metric) in metrics.iter().enumerate() {
            let price = Input::<f64>::new()
                .with_prompt(&format!("{} (GNT)", &metric))
                .interact()?;
            self.preset.usage_coeffs[idx] = price;
        }

        let price = Input::<f64>::new()
            .with_prompt(&format!("{} (GNT)", "Init price"))
            .interact()?;
        self.preset.usage_coeffs[usage_len - 1] = price;

        Ok(self.preset)
    }
}
