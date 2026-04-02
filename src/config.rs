use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WaitFor {
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub log_pattern: Option<String>,
    #[serde(default)]
    pub timeout: Option<HumanDuration>,
}

impl WaitFor {
    pub fn timeout_duration(&self) -> Option<Duration> {
        self.timeout.as_ref().map(|value| value.0)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Target {
    pub cmd: Vec<String>,
    #[serde(default)]
    pub depends: Vec<String>,
    #[serde(default)]
    pub wait_for: WaitFor,
    #[serde(default)]
    pub signal: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub stdout: Option<String>,
    #[serde(default)]
    pub stderr: Option<String>,
    #[serde(default)]
    pub fds: BTreeMap<String, String>,
}

pub type Config = BTreeMap<String, Target>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HumanDuration(pub Duration);

impl<'de> Deserialize<'de> for HumanDuration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = HumanDuration;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("duration string like \"10s\" or number of seconds")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                humantime::parse_duration(value)
                    .map(HumanDuration)
                    .map_err(E::custom)
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(HumanDuration(Duration::from_secs(value)))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(HumanDuration(Duration::from_secs_f64(value)))
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

impl Serialize for HumanDuration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&humantime::format_duration(self.0).to_string())
    }
}
