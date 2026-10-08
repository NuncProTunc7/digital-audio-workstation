//! Third-party plugins as the song stores them. Loading and running them is
//! `daw-plugins`' job; the model only remembers which plugin, its parameter
//! values, and its saved settings.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A VST3 plugin a track plays (an instrument) or runs (an effect).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PluginRef {
    /// The plugin's id, as `plugins` lists it (32 hex digits).
    pub uid: String,
    pub name: String,
    pub vendor: String,
    /// The installed `.vst3` it was loaded from on this computer.
    pub path: String,
    /// Every parameter's value (0–1) by the plugin's parameter id. Read
    /// names and meanings with plugin_params.
    #[serde(default, deserialize_with = "id_map::deserialize")]
    pub params: BTreeMap<u32, f64>,
    /// The plugin's own saved settings (base64), refreshed when the song is
    /// saved. Not for editing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// Parameter maps keyed by plugin parameter id. JSON object keys are text,
/// and not every JSON path turns `"7"` back into a number, so ids are
/// parsed here.
pub(crate) mod id_map {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, de::Error};

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<u32, f64>, D::Error> {
        BTreeMap::<String, f64>::deserialize(d)?
            .into_iter()
            .map(|(k, v)| {
                k.trim()
                    .parse::<u32>()
                    .map(|id| (id, v))
                    .map_err(|_| D::Error::custom(format!("\"{k}\" is not a parameter id")))
            })
            .collect()
    }
}
