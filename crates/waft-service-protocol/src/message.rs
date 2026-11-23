//! What a client and an app say to each other, whatever carries it: a hello, then JSON-RPC 2.0
//! requests and their responses, each one JSON text.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use serde_json::{Value, json};

/// JSON-RPC's code for text that is not JSON.
pub const PARSE_ERROR: i64 = -32700;
/// JSON-RPC's code for JSON that is not a request.
pub const INVALID_REQUEST: i64 = -32600;
/// JSON-RPC's code for a method no extension of that name serves.
pub const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC's code, and Dart's `ServiceExtensionResponse.invalidParams`, for parameters that do
/// not fit the extension.
pub const INVALID_PARAMS: i64 = -32602;
/// Dart's `ServiceExtensionResponse.extensionError`: the extension failed while answering.
pub const EXTENSION_ERROR: i64 = -32000;

/// One message.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    /// A client's first message: the token its app's listing gave.
    Hello {
        token: String,
    },
    Request(Request),
    Response(Response),
}

impl Message {
    /// The message as the JSON text a transport carries.
    pub fn to_text(&self) -> String {
        match self {
            Message::Hello { token } => json!({ "token": token }),
            Message::Request(request) => json!({
                "jsonrpc": "2.0",
                "id": request.id,
                "method": request.method,
                "params": request.params,
            }),
            Message::Response(response) => match &response.outcome {
                Ok(result) => json!({ "jsonrpc": "2.0", "id": response.id, "result": result }),
                Err(error) => {
                    json!({ "jsonrpc": "2.0", "id": response.id, "error": error.to_json() })
                }
            },
        }
        .to_string()
    }

    /// The message `text` holds, or the response a reader answers text that is none with.
    pub fn from_text(text: &str) -> Result<Message, Response> {
        let value = parsed(text).map_err(|error| {
            Response::error(Value::Null, RpcError::new(PARSE_ERROR, error.to_string()))
        })?;
        let id = value.get("id").cloned().unwrap_or(Value::Null);
        let invalid = |message: &str| {
            Response::error(
                id.clone(),
                RpcError::new(INVALID_REQUEST, message.to_owned()),
            )
        };
        if let Some(token) = value.get("token") {
            let token = token
                .as_str()
                .ok_or_else(|| invalid("a hello's token is text"))?;
            return Ok(Message::Hello {
                token: token.to_owned(),
            });
        }
        if let Some(method) = value.get("method") {
            let method = method
                .as_str()
                .ok_or_else(|| invalid("a request's method is text"))?;
            let params = match value.get("params") {
                None | Some(Value::Null) => BTreeMap::new(),
                Some(Value::Object(params)) => params
                    .iter()
                    .map(|(name, value)| (name.clone(), as_text(value)))
                    .collect(),
                Some(_) => return Err(invalid("a request's params are an object")),
            };
            return Ok(Message::Request(Request {
                id,
                method: method.to_owned(),
                params,
            }));
        }
        if let Some(result) = value.get("result") {
            return Ok(Message::Response(Response::result(id, result.clone())));
        }
        if let Some(error) = value.get("error") {
            let error = RpcError::from_json(error)
                .ok_or_else(|| invalid("an error has a code and a message"))?;
            return Ok(Message::Response(Response::error(id, error)));
        }
        Err(invalid("neither a hello, a request nor a response"))
    }
}

/// A parameter as the text Dart's `Map<String, String>` holds: a number or a flag a client sent
/// as JSON is passed as it prints.
fn as_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// One call: JSON-RPC's request, an extension's name as its method.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub id: Value,
    pub method: String,
    pub params: BTreeMap<String, String>,
}

/// The answer to one request.
#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    pub id: Value,
    pub outcome: Result<Value, RpcError>,
}

impl Response {
    pub fn result(id: Value, result: Value) -> Response {
        Response {
            id,
            outcome: Ok(result),
        }
    }

    pub fn error(id: Value, error: RpcError) -> Response {
        Response {
            id,
            outcome: Err(error),
        }
    }
}

/// A JSON-RPC error: its code, a message for a person, and what else the app knows.
#[derive(Clone, Debug, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i64, message: String) -> RpcError {
        RpcError {
            code,
            message,
            data: None,
        }
    }

    pub fn with_data(mut self, data: Value) -> RpcError {
        self.data = Some(data);
        self
    }

    fn to_json(&self) -> Value {
        match &self.data {
            Some(data) => json!({ "code": self.code, "message": self.message, "data": data }),
            None => json!({ "code": self.code, "message": self.message }),
        }
    }

    fn from_json(value: &Value) -> Option<RpcError> {
        Some(RpcError {
            code: value.get("code")?.as_i64()?,
            message: value.get("message")?.as_str()?.to_owned(),
            data: value.get("data").cloned(),
        })
    }
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RpcError {}

/// The JSON `text` holds, however deep: a message is as deep as the widget tree an app writes into
/// it, two levels for each of the tree's, and a real app's is deeper than `serde_json`'s default
/// limit of 128. The app wrote it out by recursing that deep, so reading it back can too.
fn parsed(text: &str) -> serde_json::Result<Value> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    deserializer.disable_recursion_limit();
    let value = Value::deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(message: Message) {
        assert_eq!(Message::from_text(&message.to_text()), Ok(message));
    }

    #[test]
    fn each_message_reads_back_as_written() {
        round_trip(Message::Hello {
            token: "9f2c".to_owned(),
        });
        round_trip(Message::Request(Request {
            id: json!(1),
            method: "ext.waft.debugDumpApp".to_owned(),
            params: BTreeMap::from([("id".to_owned(), "inspector-1".to_owned())]),
        }));
        round_trip(Message::Response(Response::result(
            json!(1),
            json!({ "data": "tree" }),
        )));
        round_trip(Message::Response(Response::error(
            json!(2),
            RpcError::new(METHOD_NOT_FOUND, "no such extension".to_owned())
                .with_data(json!({ "registered": ["ext.waft.debugDumpApp"] })),
        )));
    }

    /// A parameter a client sent as a number reaches the extension as the text it prints.
    #[test]
    fn a_request_carries_its_params_as_text() {
        let message = Message::from_text(
            r#"{"jsonrpc":"2.0","id":7,"method":"ext.waft.inspector.screenshot","params":{"maxPixelRatio":2}}"#,
        );
        let Ok(Message::Request(request)) = message else {
            panic!("a request: {message:?}");
        };
        assert_eq!(request.params["maxPixelRatio"], "2");
    }

    #[test]
    fn text_that_is_no_message_is_answered_with_jsonrpcs_errors() {
        let error = |text| {
            Message::from_text(text)
                .expect_err("no message")
                .outcome
                .expect_err("an error")
        };
        assert_eq!(error("{").code, PARSE_ERROR);
        assert_eq!(error(r#"{"id":1}"#).code, INVALID_REQUEST);
        assert_eq!(
            Message::from_text(r#"{"id":1}"#)
                .expect_err("no message")
                .id,
            json!(1)
        );
    }

    /// A widget tree nests two levels of JSON for each of its own, so a real app's is deeper
    /// than a parser's usual limit of 128.
    #[test]
    fn a_deep_tree_reads_back() {
        let mut tree = json!({ "description": "Text" });
        for _ in 0..300 {
            tree = json!({ "description": "Padding", "children": [tree] });
        }
        let answer = Message::Response(Response::result(json!(7), json!({ "result": tree })));
        assert_eq!(Message::from_text(&answer.to_text()), Ok(answer));
    }
}
