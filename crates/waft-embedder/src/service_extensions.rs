//! What a service extension call carries between a host and the framework: its parameters, and
//! its answer or the reason it has none.
//!
//! Flutter's service extensions are `dart:developer`'s: a method name beginning `ext.`, string
//! parameters, and a JSON answer, which the Dart VM service carries to a client. Here the
//! framework keeps the extensions (`waft_foundation::ServiceExtensions`) and a host carries the
//! calls, so these are the types both sides read.

use std::collections::BTreeMap;
use std::fmt;

/// One call's parameters: Dart's `Map<String, String>`, as a client sends them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServiceExtensionParams(BTreeMap<String, String>);

impl ServiceExtensionParams {
    /// The parameters `pairs` names.
    pub fn new(pairs: impl IntoIterator<Item = (String, String)>) -> ServiceExtensionParams {
        ServiceExtensionParams(pairs.into_iter().collect())
    }

    /// The value of `name`, when the client sent it.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }

    /// The value of `name`, or the error a client is answered when a required one is missing.
    pub fn require(&self, name: &str) -> Result<&str, ServiceExtensionError> {
        self.get(name)
            .ok_or_else(|| ServiceExtensionError::InvalidParams(format!("`{name}` is required")))
    }

    /// Every parameter, by name.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }
}

/// What a service extension answers: Dart's `ServiceExtensionResponse.result`, the JSON a
/// callback's map encodes to, or the reason there is none.
pub type ServiceExtensionResult = Result<serde_json::Value, ServiceExtensionError>;

/// Why a call has no answer: the errors Dart's `ServiceExtensionResponse.error` reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServiceExtensionError {
    /// No extension of that name is registered; `registered` names those that are, so a client
    /// can tell what this app serves.
    Unknown {
        name: String,
        registered: Vec<String>,
    },
    /// The parameters do not fit the extension: Dart's `invalidParams`.
    InvalidParams(String),
    /// The extension failed while answering: Dart's `extensionError`.
    Failed(String),
}

impl fmt::Display for ServiceExtensionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServiceExtensionError::Unknown { name, registered } => write!(
                f,
                "no service extension `{name}` is registered; this app serves {}",
                registered.join(", ")
            ),
            ServiceExtensionError::InvalidParams(reason) => {
                write!(f, "invalid parameters: {reason}")
            }
            ServiceExtensionError::Failed(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for ServiceExtensionError {}
