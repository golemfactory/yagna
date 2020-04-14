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
            .interact()?;
        self.preset.exeunit_name = self.exeunits[exeunit_idx].clone();

        let pricing_idx = Select::new()
            .with_prompt("ExeUnit")
            .items(&self.pricing_models[..])
            .interact()?;
        self.preset.pricing_model = self.pricing_models[pricing_idx].clone();

        Ok(self.preset)
    }
}
