use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::str::FromStr;

use anyhow::anyhow;
use bigdecimal::BigDecimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;

use ya_utils_path::SwapSave;

use crate::market::Preset;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PresetV0 {
    pub name: String,
    pub exeunit_name: String,
    pub pricing_model: String,
    #[serde(with = "prices_serde")]
    pub usage_coeffs: BTreeMap<String, BigDecimal>,
}

#[derive(Deserialize)]
struct PresetsFileV0 {
    active: Vec<String>,
    presets: Vec<PresetV0>,
}

#[derive(Deserialize)]
struct PresetsFileV1 {
    active: Vec<String>,
    presets: Vec<Preset>,
}

/// Exact decimal representation used at JSON boundaries.
///
/// `bigdecimal/serde-json` cannot be used here because it enables
/// `serde_json/arbitrary_precision` for the whole process. That feature changes
/// how every `serde_json::Number` is serialized by non-JSON serializers.
pub mod json_decimal {
    use super::*;

    pub fn serialize<S>(value: &BigDecimal, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let raw =
            RawValue::from_string(value.to_plain_string()).map_err(serde::ser::Error::custom)?;
        raw.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<BigDecimal, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        let value = if raw.get().starts_with('"') {
            serde_json::from_str::<String>(raw.get()).map_err(serde::de::Error::custom)?
        } else {
            raw.get().to_string()
        };

        BigDecimal::from_str(&value).map_err(serde::de::Error::custom)
    }

    pub fn from_value(value: &serde_json::Value) -> Result<BigDecimal, String> {
        let value = match value {
            serde_json::Value::Number(number) => number.to_string(),
            serde_json::Value::String(value) => value.clone(),
            value => return Err(format!("expected a JSON number or string, got {value}")),
        };

        BigDecimal::from_str(&value).map_err(|error| error.to_string())
    }

    pub fn vec_from_value(value: &serde_json::Value) -> Result<Vec<BigDecimal>, String> {
        value
            .as_array()
            .ok_or_else(|| format!("expected a JSON array, got {value}"))?
            .iter()
            .map(from_value)
            .collect()
    }
}

pub mod prices_serde {
    use super::*;
    use serde::ser::SerializeMap;

    struct JsonDecimalRef<'a>(&'a BigDecimal);

    impl Serialize for JsonDecimalRef<'_> {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            json_decimal::serialize(self.0, serializer)
        }
    }

    struct JsonDecimal(BigDecimal);

    impl<'de> Deserialize<'de> for JsonDecimal {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            json_decimal::deserialize(deserializer).map(Self)
        }
    }

    pub fn serialize<S>(
        prices: &BTreeMap<String, BigDecimal>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(prices.len()))?;
        for (name, price) in prices {
            map.serialize_entry(name, &JsonDecimalRef(price))?;
        }
        map.end()
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<BTreeMap<String, BigDecimal>, D::Error>
    where
        D: Deserializer<'de>,
    {
        BTreeMap::<String, JsonDecimal>::deserialize(deserializer).map(|prices| {
            prices
                .into_iter()
                .map(|(name, price)| (name, price.0))
                .collect()
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "kebab-case")]
pub struct Presets {
    pub active: Vec<String>,
    // It's important that all values are sorted, so that other tools can easily detect changes.
    pub presets: BTreeMap<String, Preset>,
}

#[derive(Serialize)]
#[serde(tag = "ver")]
enum PresetsFile {
    V0 {
        active: Vec<String>,
        presets: Vec<PresetV0>,
    },
    V1 {
        active: Vec<String>,
        presets: Vec<Preset>,
    },
}

impl Presets {
    pub fn load_from_file<P: AsRef<Path>>(presets_file: P) -> anyhow::Result<Presets> {
        let path = presets_file.as_ref();
        log::debug!("Loading presets from: {}", path.display());
        let json = std::fs::read_to_string(path)?;
        let header: serde_json::Value = serde_json::from_str(&json)?;
        let presets_file = match header.get("ver").and_then(serde_json::Value::as_str) {
            Some("V1") => {
                serde_json::from_str::<PresetsFileV1>(&json).map(|file| PresetsFile::V1 {
                    active: file.active,
                    presets: file.presets,
                })
            }
            None | Some("V0") => {
                serde_json::from_str::<PresetsFileV0>(&json).map(|legacy| PresetsFile::V0 {
                    active: legacy.active,
                    presets: legacy.presets,
                })
            }
            Some(version) => return Err(anyhow!("Unsupported presets file version: {version}")),
        };

        let presets: Presets = presets_file
            .map_err(|e| anyhow!("Can't deserialize Presets from file {:?}: {}", path, e))?
            .into();

        presets.active.iter().try_for_each(|name| {
            presets
                .presets
                .get(name)
                .ok_or_else(|| anyhow!("Invalid active preset: {:?}", name))
                .map(|_| ())
        })?;

        Ok(presets)
    }

    pub fn save_to_file(&self, presets_file: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(&PresetsFile::from(self))
            .map_err(|error| anyhow!("Failed to serialize Presets: {}", error))?;
        presets_file.swap_save(json).map_err(|error| {
            anyhow!(
                "Failed to save Presets to file {}, error: {}.",
                presets_file.display(),
                error
            )
        })?;
        Ok(())
    }

    pub fn diff(&self, other: &Presets) -> (Vec<String>, Vec<String>) {
        let mut updated = HashSet::new();
        let mut removed = HashSet::new();

        self.active.iter().for_each(|n| {
            if !other.active.contains(n) {
                removed.insert(n.clone());
            }
        });
        self.presets
            .iter()
            .for_each(|(n, p)| match other.presets.get(n) {
                Some(preset) => {
                    if preset != p {
                        updated.insert(n.clone());
                    }
                }
                _ => {
                    removed.insert(n.clone());
                }
            });

        (updated.into_iter().collect(), removed.into_iter().collect())
    }
}

impl From<PresetsFile> for Presets {
    fn from(presets_file: PresetsFile) -> Self {
        match presets_file {
            PresetsFile::V0 { active, presets } => Presets {
                active,
                presets: presets
                    .into_iter()
                    .map(|p: PresetV0| (p.name.clone(), p.into()))
                    .collect(),
            },
            PresetsFile::V1 { active, presets } => Presets {
                active,
                presets: presets.into_iter().map(|p| (p.name.clone(), p)).collect(),
            },
        }
    }
}

impl<'p> From<&'p Presets> for PresetsFile {
    fn from(presets: &'p Presets) -> Self {
        PresetsFile::V1 {
            active: presets.active.clone(),
            presets: presets.presets.values().cloned().collect(),
        }
    }
}

impl From<PresetV0> for Preset {
    fn from(old_preset: PresetV0) -> Self {
        Preset {
            name: old_preset.name,
            exeunit_name: old_preset.exeunit_name,
            pricing_model: old_preset.pricing_model,
            initial_price: old_preset
                .usage_coeffs
                .get("initial")
                .cloned()
                .unwrap_or_else(|| BigDecimal::from(0)),
            usage_coeffs: old_preset
                .usage_coeffs
                .into_iter()
                .filter_map(|(name, price)| match name.as_str() {
                    "duration" => Some(("golem.usage.duration_sec".to_string(), price)),
                    "cpu" => Some(("golem.usage.cpu_sec".to_string(), price)),
                    _ => None,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn load(json: &str) -> Presets {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("presets.json");
        std::fs::write(&path, json).unwrap();
        Presets::load_from_file(path).unwrap()
    }

    #[test]
    fn loads_legacy_v0_numeric_prices_exactly() {
        let presets = load(
            r#"{
                "active": ["vm"],
                "presets": [{
                    "name": "vm",
                    "exeunit-name": "vm",
                    "pricing-model": "linear",
                    "usage-coeffs": {
                        "initial": 0.2,
                        "duration": 0.0001,
                        "cpu": 0.0002
                    }
                }]
            }"#,
        );

        let preset = presets.presets.get("vm").unwrap();
        assert_eq!(preset.initial_price, BigDecimal::from_str("0.2").unwrap());
        assert_eq!(
            preset.usage_coeffs["golem.usage.duration_sec"],
            BigDecimal::from_str("0.0001").unwrap()
        );
        assert_eq!(
            preset.usage_coeffs["golem.usage.cpu_sec"],
            BigDecimal::from_str("0.0002").unwrap()
        );
    }

    #[test]
    fn loads_and_saves_v1_prices_as_exact_json_numbers() {
        let presets = load(
            r#"{
                "ver": "V1",
                "active": ["vm"],
                "presets": [{
                    "name": "vm",
                    "exeunit-name": "vm",
                    "pricing-model": "linear",
                    "initial-price": 0.12345678901234567890123456789,
                    "usage-coeffs": {
                        "golem.usage.duration_sec": 0.000000000000000001,
                        "golem.usage.cpu_sec": 0.0002
                    }
                }]
            }"#,
        );

        let preset = presets.presets.get("vm").unwrap();
        assert_eq!(
            preset.initial_price,
            BigDecimal::from_str("0.12345678901234567890123456789").unwrap()
        );
        assert_eq!(
            preset.usage_coeffs["golem.usage.duration_sec"],
            BigDecimal::from_str("0.000000000000000001").unwrap()
        );

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("presets.json");
        presets.save_to_file(&path).unwrap();
        let saved = std::fs::read_to_string(path).unwrap();
        assert!(saved.contains("\"initial-price\": 0.12345678901234567890123456789"));
        let saved_json: serde_json::Value = serde_json::from_str(&saved).unwrap();
        assert!(saved_json["presets"][0]["initial-price"].is_number());
        assert!(saved_json["presets"][0]["usage-coeffs"]["golem.usage.duration_sec"].is_number());

        let reloaded = load(&saved);
        let reloaded_preset = reloaded.presets.get("vm").unwrap();
        assert_eq!(reloaded_preset.initial_price, preset.initial_price);
        assert_eq!(reloaded_preset.usage_coeffs, preset.usage_coeffs);
    }
}
