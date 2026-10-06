pub use crate::integrations::Client;

use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StackV1 {
    pub schema_version: u64,
    #[serde(deserialize_with = "unique_map")]
    pub servers: BTreeMap<String, Server>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(untagged)]
pub enum Server {
    Portable {
        transport: Transport,
        #[serde(default)]
        settings: Settings,
    },
    ClientSpecific {
        client: Client,
        #[serde(deserialize_with = "unique_map")]
        config: BTreeMap<String, ClientValue>,
    },
}

/// Native configuration data, not permission to write it to a client.
/// Recursive objects reject duplicate keys; {"$env":"NAME"} is reserved
/// for secret references at any depth, including inside arrays.
#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub enum ClientValue {
    Object(#[serde(deserialize_with = "unique_map")] BTreeMap<String, ClientValue>),
    Array(Vec<ClientValue>),
    String(String),
    Number(serde_json::Number),
    Bool(bool),
    Null(()),
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Transport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<ValueSource>,
        #[serde(default, deserialize_with = "unique_map")]
        env: BTreeMap<String, ValueSource>,
        #[serde(default)]
        env_vars: Vec<String>,
        #[serde(default)]
        cwd: Option<String>,
    },
    Http {
        url: String,
        #[serde(default)]
        bearer_token: Option<EnvironmentReference>,
        #[serde(default, deserialize_with = "unique_map")]
        headers: BTreeMap<String, ValueSource>,
    },
    Sse {
        url: String,
        #[serde(default)]
        bearer_token: Option<EnvironmentReference>,
        #[serde(default, deserialize_with = "unique_map")]
        headers: BTreeMap<String, ValueSource>,
    },
    Websocket {
        url: String,
        #[serde(default)]
        bearer_token: Option<EnvironmentReference>,
        #[serde(default, deserialize_with = "unique_map")]
        headers: BTreeMap<String, ValueSource>,
    },
}

/// Explicit references cannot be confused with literal strings.
#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub enum ValueSource {
    Literal(String),
    Environment(EnvironmentReference),
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentReference {
    pub env: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub startup_timeout_sec: Option<f64>,
    #[serde(default)]
    pub tool_timeout_sec: Option<f64>,
    #[serde(default)]
    pub enabled_tools: Option<Vec<String>>,
    #[serde(default)]
    pub disabled_tools: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            required: false,
            startup_timeout_sec: None,
            tool_timeout_sec: None,
            enabled_tools: None,
            disabled_tools: Vec::new(),
        }
    }
}

fn enabled_by_default() -> bool {
    true
}

// Serde's standard map decoder silently replaces duplicate keys. Reject them.
fn unique_map<'de, D, V>(deserializer: D) -> Result<BTreeMap<String, V>, D::Error>
where
    D: Deserializer<'de>,
    V: Deserialize<'de>,
{
    struct UniqueMap<V>(PhantomData<V>);
    impl<'de, V: Deserialize<'de>> Visitor<'de> for UniqueMap<V> {
        type Value = BTreeMap<String, V>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an object with unique keys")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut values = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, V>()? {
                if values.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom("duplicate object key"));
                }
            }
            Ok(values)
        }
    }
    deserializer.deserialize_map(UniqueMap(PhantomData))
}

use super::{SCHEMA_VERSION, ValidationError};

impl StackV1 {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ValidationError::UnsupportedVersion);
        }
        for (name, server) in &self.servers {
            if !nonempty_text(name) || name.trim() != name {
                return Err(ValidationError::InvalidServerName);
            }
            server.validate()?;
        }
        Ok(())
    }
}

impl Server {
    fn validate(&self) -> Result<(), ValidationError> {
        let (transport, settings) = match self {
            Self::Portable {
                transport,
                settings,
            } => (transport, settings),
            Self::ClientSpecific { config, .. } => {
                return config.values().try_for_each(ClientValue::validate);
            }
        };
        settings.validate()?;
        match transport {
            Transport::Stdio {
                command,
                args,
                env,
                env_vars,
                cwd,
            } => {
                if !nonempty_text(command) || cwd.as_ref().is_some_and(|path| !nonempty_text(path))
                {
                    return Err(ValidationError::InvalidProcess);
                }
                for value in args.iter().chain(env.values()) {
                    value.validate_reference()?;
                }
                if !env.keys().all(|name| environment_name(name))
                    || !env_vars.iter().all(|name| environment_name(name))
                    || !unique(env_vars)
                {
                    return Err(ValidationError::InvalidEnvironment);
                }
            }
            Transport::Http {
                url,
                bearer_token,
                headers,
            }
            | Transport::Sse {
                url,
                bearer_token,
                headers,
            }
            | Transport::Websocket {
                url,
                bearer_token,
                headers,
            } => {
                let endpoint =
                    url::Url::parse(url).map_err(|_| ValidationError::InvalidEndpoint)?;
                let valid_scheme = match transport {
                    Transport::Websocket { .. } => matches!(endpoint.scheme(), "ws" | "wss"),
                    _ => matches!(endpoint.scheme(), "http" | "https"),
                };
                if !valid_scheme
                    || endpoint.host_str().is_none()
                    || !endpoint.username().is_empty()
                    || endpoint.password().is_some()
                    || endpoint.fragment().is_some()
                    || url.chars().any(char::is_control)
                    || url.trim() != url
                {
                    return Err(ValidationError::InvalidEndpoint);
                }
                if let Some(reference) = bearer_token {
                    reference.validate()?;
                }
                let mut header_names = std::collections::HashSet::new();
                for (name, value) in headers {
                    if name.is_empty()
                        || !name
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c))
                        || !header_names.insert(name.to_ascii_lowercase())
                        || matches!(value, ValueSource::Literal(text) if text.chars().any(char::is_control))
                    {
                        return Err(ValidationError::InvalidHeader);
                    }
                    value.validate_reference()?;
                }
            }
        }
        Ok(())
    }
}

impl ClientValue {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Object(values) if values.contains_key("$env") => {
                if values.len() != 1 {
                    return Err(ValidationError::InvalidSecretReference);
                }
                match &values["$env"] {
                    Self::String(name) if environment_name(name) => Ok(()),
                    _ => Err(ValidationError::InvalidSecretReference),
                }
            }
            Self::Object(values) => values.values().try_for_each(Self::validate),
            Self::Array(values) => values.iter().try_for_each(Self::validate),
            Self::String(value) if value.contains('\0') => Err(ValidationError::InvalidValue),
            _ => Ok(()),
        }
    }
}

impl Settings {
    fn validate(&self) -> Result<(), ValidationError> {
        for timeout in [self.startup_timeout_sec, self.tool_timeout_sec]
            .into_iter()
            .flatten()
        {
            if !timeout.is_finite() || timeout <= 0.0 {
                return Err(ValidationError::InvalidTimeout);
            }
        }
        for tools in [
            self.enabled_tools.as_deref().unwrap_or_default(),
            &self.disabled_tools,
        ] {
            if !tools
                .iter()
                .all(|name| nonempty_text(name) && name.trim() == name)
                || !unique(tools)
            {
                return Err(ValidationError::InvalidTools);
            }
        }
        Ok(())
    }
}

impl EnvironmentReference {
    fn validate(&self) -> Result<(), ValidationError> {
        if !environment_name(&self.env) {
            return Err(ValidationError::InvalidSecretReference);
        }
        Ok(())
    }
}

impl ValueSource {
    fn validate_reference(&self) -> Result<(), ValidationError> {
        match self {
            Self::Environment(reference) => reference.validate(),
            Self::Literal(value) if value.contains('\0') => Err(ValidationError::InvalidValue),
            Self::Literal(_) => Ok(()),
        }
    }
}

fn nonempty_text(value: &str) -> bool {
    !value.trim().is_empty() && !value.chars().any(char::is_control)
}

fn environment_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

fn unique(values: &[String]) -> bool {
    let mut seen = std::collections::HashSet::new();
    values.iter().all(|value| seen.insert(value))
}
