//! What a service extension call carries between a host and the framework: its parameters, and
//! its answer or the reason it has none.
//!
//! Flutter's service extensions are `dart:developer`'s: a method name beginning `ext.`, string
//! parameters, and a JSON answer, which the Dart VM service carries to a client. Here the
//! framework keeps the extensions (`waft_foundation::ServiceExtensions`) and a host carries the
//! calls ([`EmbedderClient::service_extension_call`](crate::EmbedderClient::service_extension_call)),
//! so these are the types both sides read.

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

/// One call from a client, which a host hands the framework through
/// [`EmbedderClient::service_extension_call`](crate::EmbedderClient::service_extension_call).
pub struct ServiceExtensionCall {
    /// The extension's whole name, as `ext.waft.debugDumpApp`.
    pub name: String,
    pub params: ServiceExtensionParams,
    /// Where the answer goes.
    pub reply: ServiceExtensionReply,
}

/// The answer to one call, given once, as [`AppExitReply`](crate::AppExitReply) is. Dropped
/// unanswered, it answers [`ServiceExtensionError::Failed`], so a client is never left waiting.
pub struct ServiceExtensionReply(Option<Box<dyn FnOnce(ServiceExtensionResult)>>);

impl ServiceExtensionReply {
    /// A reply that hands its answer to `send`. The answer is given inside a call into the
    /// client, so `send` may only post it to the host's own loop or thread, never call the
    /// client.
    pub fn new(send: impl FnOnce(ServiceExtensionResult) + 'static) -> ServiceExtensionReply {
        ServiceExtensionReply(Some(Box::new(send)))
    }

    pub fn answer(mut self, result: ServiceExtensionResult) {
        self.send(result);
    }

    fn send(&mut self, result: ServiceExtensionResult) {
        if let Some(send) = self.0.take() {
            send(result);
        }
    }
}

impl Drop for ServiceExtensionReply {
    fn drop(&mut self) {
        self.send(Err(ServiceExtensionError::Failed(
            "the app let go of the call without answering it".to_owned(),
        )));
    }
}

impl fmt::Debug for ServiceExtensionReply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceExtensionReply")
            .field("answered", &self.0.is_none())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    #[test]
    fn a_reply_dropped_unanswered_answers_that_it_failed() {
        let answered = Rc::new(RefCell::new(None));
        let to = Rc::clone(&answered);
        drop(ServiceExtensionReply::new(move |result| {
            *to.borrow_mut() = Some(result)
        }));
        assert!(matches!(
            answered.borrow().as_ref(),
            Some(Err(ServiceExtensionError::Failed(_)))
        ));
    }

    #[test]
    fn a_reply_answers_once() {
        let answers = Rc::new(RefCell::new(Vec::new()));
        let to = Rc::clone(&answers);
        ServiceExtensionReply::new(move |result| to.borrow_mut().push(result))
            .answer(Ok(serde_json::json!({ "data": "tree" })));
        assert_eq!(
            *answers.borrow(),
            [Ok(serde_json::json!({ "data": "tree" }))]
        );
    }
}
