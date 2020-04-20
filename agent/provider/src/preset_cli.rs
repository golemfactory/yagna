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

    pub fn update_exeunit(&mut self) -> Result<()> {
        let prev_exeunit = self
            .exeunits
            .iter()
            .position(|exeunit| exeunit == &self.preset.exeunit_name)
            .unwrap_or(0);

        let exeunit_idx = Select::new()
            .with_prompt("ExeUnit")
            .items(&self.exeunits[..])
            .default(prev_exeunit)
            .interact()?;
        self.preset.exeunit_name = self.exeunits[exeunit_idx].clone();
        Ok(())
    }

    pub fn update_pricing_model(&mut self) -> Result<()> {
        let prev_pricing = self
            .pricing_models
            .iter()
            .position(|pricing| pricing == &self.preset.pricing_model)
            .unwrap_or(0);

        let pricing_idx = Select::new()
            .with_prompt("Pricing model")
            .items(&self.pricing_models[..])
            .default(prev_pricing)
            .interact()?;
        self.preset.pricing_model = self.pricing_models[pricing_idx].clone();
        Ok(())
    }

    pub fn update_metrics(&mut self) -> Result<()> {
        let metrics = self.preset.list_readable_metrics();
        let usage_len = metrics.len() + 1;
        self.preset.usage_coeffs.resize(usage_len, 0.0);

        for (idx, metric) in metrics.iter().enumerate() {
            let prev_price = self.preset.usage_coeffs[idx];
            let price = Input::<f64>::new()
                .with_prompt(&format!("{} (GNT)", &metric))
                .default(prev_price)
                .show_default(true)
                .interact()?;
            self.preset.usage_coeffs[idx] = price;
        }

        let prev_price = self.preset.usage_coeffs[usage_len - 1];
        let price = Input::<f64>::new()
            .with_prompt(&format!("{} (GNT)", "Init price"))
            .default(prev_price)
            .show_default(true)
            .interact()?;
        self.preset.usage_coeffs[usage_len - 1] = price;
        Ok(())
    }

    pub fn update_name(&mut self) -> Result<()> {
        self.preset.name = Input::<String>::new()
            .with_prompt("Preset name")
            .default(self.preset.name.clone())
            .show_default(true)
            .interact()?;
        Ok(())
    }

    pub fn interact(mut self) -> Result<Preset> {
        self.update_name()?;
        self.update_exeunit()?;
        self.update_pricing_model()?;
        self.update_metrics()?;

        Ok(self.preset)
    }
}
