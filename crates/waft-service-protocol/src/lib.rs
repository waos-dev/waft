//! How a client reaches the service extensions of a Waft app running on this machine: where a
//! running app lists itself, and what the two say to each other.
//!
//! Flutter's tools reach an app through the Dart VM service: a WebSocket on the loopback
//! interface, behind a token in its address, speaking JSON-RPC 2.0 with an extension's name as
//! the method. A Waft host serves the same in a debug build, without the WebSocket:
//!
//! 1. **Listing.** The host listens on a loopback TCP port and writes a [`ListedApp`] (its app's
//!    name, process, port and a token) into [`listing_dir`], a folder only its user can read, and
//!    removes it when the app ends. A client reads the folder ([`ListedApp::all`]) to find the
//!    apps running.
//! 2. **Hello.** A client connects and sends the token as its first line, `{"token": "…"}`; the
//!    host closes a connection whose first line does not carry it, so another user on the
//!    machine cannot call into the app.
//! 3. **Calls.** Then one JSON-RPC 2.0 request per line, `{"jsonrpc": "2.0", "id": 1, "method":
//!    "ext.waft.debugDumpApp", "params": {…}}`, each answered by one line with its `result` or its
//!    `error`, in the order asked. An error's code is Dart's: [`METHOD_NOT_FOUND`],
//!    [`INVALID_PARAMS`] or [`EXTENSION_ERROR`].

use std::collections::BTreeMap;
use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// JSON-RPC's code for a method no extension of that name serves.
pub const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC's code, and Dart's `ServiceExtensionResponse.invalidParams`, for parameters that do
/// not fit the extension.
pub const INVALID_PARAMS: i64 = -32602;
/// Dart's `ServiceExtensionResponse.extensionError`: the extension failed while answering.
pub const EXTENSION_ERROR: i64 = -32000;

/// The folder running apps list themselves in: under the user's runtime directory where the
/// system has one (`$XDG_RUNTIME_DIR`), else under the user's temporary folder, apart for each
/// user.
pub fn listing_dir() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(runtime) => PathBuf::from(runtime).join("waft"),
        None => {
            let user = std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_default();
            std::env::temp_dir().join(format!("waft-{user}"))
        }
    }
}

/// A running app, as it lists itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedApp {
    /// The app's name: its executable's, as `waos`.
    pub app: String,
    pub pid: u32,
    /// The loopback port it listens on.
    pub port: u16,
    /// What a client's first line must carry.
    pub token: String,
}

impl ListedApp {
    /// The app `app`, process `pid`, listening on `port`, with a new token.
    pub fn new(app: impl Into<String>, pid: u32, port: u16) -> ListedApp {
        ListedApp {
            app: app.into(),
            pid,
            port,
            token: new_token(),
        }
    }

    /// Every app listed in `dir`, by name then process. A file that cannot be read as a listing
    /// is passed over.
    pub fn all_in(dir: &Path) -> Vec<ListedApp> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut apps: Vec<ListedApp> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
            .filter_map(|text| ListedApp::from_json(&text))
            .collect();
        apps.sort_by(|a, b| (&a.app, a.pid).cmp(&(&b.app, b.pid)));
        apps
    }

    /// Every app listed in [`listing_dir`].
    pub fn all() -> Vec<ListedApp> {
        ListedApp::all_in(&listing_dir())
    }

    /// Lists this app in `dir`, readable by the user alone, and answers the file written,
    /// which the app removes when it ends.
    pub fn write_in(&self, dir: &Path) -> io::Result<PathBuf> {
        create_private_dir(dir)?;
        let path = dir.join(format!("{}-{}.json", self.app, self.pid));
        let mut file = private_file(&path)?;
        file.write_all(self.to_json().to_string().as_bytes())?;
        Ok(path)
    }

    fn to_json(&self) -> Value {
        json!({ "app": self.app, "pid": self.pid, "port": self.port, "token": self.token })
    }

    fn from_json(text: &str) -> Option<ListedApp> {
        let value: Value = serde_json::from_str(text).ok()?;
        Some(ListedApp {
            app: value.get("app")?.as_str()?.to_owned(),
            pid: u32::try_from(value.get("pid")?.as_u64()?).ok()?,
            port: u16::try_from(value.get("port")?.as_u64()?).ok()?,
            token: value.get("token")?.as_str()?.to_owned(),
        })
    }
}

/// 128 random bits as hex: std's hasher keys are seeded from the system's randomness.
fn new_token() -> String {
    let half = || {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(u64::from(std::process::id()));
        hasher.finish()
    };
    format!("{:016x}{:016x}", half(), half())
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)
}

#[cfg(unix)]
fn private_file(path: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn private_file(path: &Path) -> io::Result<std::fs::File> {
    std::fs::File::create(path)
}

/// A client's first line: the token it was given.
pub fn hello(token: &str) -> String {
    json!({ "token": token }).to_string()
}

/// Whether `line`, a client's first, carries `token`.
pub fn is_hello_for(line: &str, token: &str) -> bool {
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|value| value.get("token")?.as_str().map(|sent| sent == token))
        .unwrap_or(false)
}

/// One call, as a request line names it.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub id: Value,
    pub method: String,
    /// Each parameter as text, as Dart's extensions take them: a number or a flag a client sent
    /// as JSON is passed as it prints.
    pub params: BTreeMap<String, String>,
}

impl Request {
    /// The request on `line`, or the error to answer it with.
    pub fn parse(line: &str) -> Result<Request, Response> {
        let invalid = |id: Value, message: &str| {
            Response::error(id, RpcError::new(-32600, message.to_owned()))
        };
        let value: Value = serde_json::from_str(line).map_err(|error| {
            Response::error(Value::Null, RpcError::new(-32700, error.to_string()))
        })?;
        let id = value.get("id").cloned().unwrap_or(Value::Null);
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid(id.clone(), "a request names its method"))?
            .to_owned();
        let params = match value.get("params") {
            None | Some(Value::Null) => BTreeMap::new(),
            Some(Value::Object(params)) => params
                .iter()
                .map(|(name, value)| (name.clone(), as_text(value)))
                .collect(),
            Some(_) => return Err(invalid(id, "a request's params are an object")),
        };
        Ok(Request { id, method, params })
    }

    /// The line a client sends for this request.
    pub fn to_line(&self) -> String {
        json!({ "jsonrpc": "2.0", "id": self.id, "method": self.method, "params": self.params })
            .to_string()
    }
}

/// A parameter as the text Dart's `Map<String, String>` holds.
fn as_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
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

    /// The line the host answers with.
    pub fn to_line(&self) -> String {
        match &self.outcome {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": self.id, "result": result }),
            Err(error) => json!({ "jsonrpc": "2.0", "id": self.id, "error": error.to_json() }),
        }
        .to_string()
    }

    /// The response on `line`, as a client reads it.
    pub fn parse(line: &str) -> Option<Response> {
        let value: Value = serde_json::from_str(line).ok()?;
        let id = value.get("id").cloned().unwrap_or(Value::Null);
        if let Some(result) = value.get("result") {
            return Some(Response::result(id, result.clone()));
        }
        let error = value.get("error")?;
        Some(Response::error(
            id,
            RpcError {
                code: error.get("code")?.as_i64()?,
                message: error.get("message")?.as_str()?.to_owned(),
                data: error.get("data").cloned(),
            },
        ))
    }
}

/// A JSON-RPC error: its code, a message for a person, and what else the host knows.
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
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RpcError {}

/// A client's connection to one running app.
pub struct Client {
    lines: BufReader<TcpStream>,
    next_id: u64,
}

impl Client {
    /// Connects to `app` and says hello. An app that ended since it listed itself refuses the
    /// connection.
    pub fn connect(app: &ListedApp) -> io::Result<Client> {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, app.port))?;
        writeln!(stream, "{}", hello(&app.token))?;
        Ok(Client {
            lines: BufReader::new(stream),
            next_id: 1,
        })
    }

    /// Calls `method` with `params` and waits for its answer.
    pub fn call(
        &mut self,
        method: &str,
        params: BTreeMap<String, String>,
    ) -> io::Result<Result<Value, RpcError>> {
        let request = Request {
            id: json!(self.next_id),
            method: method.to_owned(),
            params,
        };
        self.next_id += 1;
        writeln!(self.lines.get_mut(), "{}", request.to_line())?;
        let mut line = String::new();
        if self.lines.read_line(&mut line)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the app closed the connection",
            ));
        }
        Response::parse(&line)
            .map(|response| response.outcome)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "the app's answer is not JSON-RPC",
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_reads_back_as_written() {
        let dir = std::env::temp_dir().join(format!("waft-listing-test-{}", std::process::id()));
        let app = ListedApp::new("notes", 42, 9000);
        let path = app.write_in(&dir).expect("written");
        assert_eq!(ListedApp::all_in(&dir), std::slice::from_ref(&app));
        std::fs::remove_file(path).expect("removed");
        std::fs::remove_dir(&dir).expect("removed");
    }

    #[test]
    fn a_hello_carries_its_token_alone() {
        let token = new_token();
        assert_eq!(token.len(), 32);
        assert!(is_hello_for(&hello(&token), &token));
        assert!(!is_hello_for(&hello("guess"), &token));
        assert!(!is_hello_for("not json", &token));
    }

    /// A parameter a client sent as a number reaches the extension as the text it prints, as
    /// Dart's string map holds it.
    #[test]
    fn a_request_carries_its_params_as_text() {
        let request = Request::parse(
            r#"{"jsonrpc":"2.0","id":7,"method":"ext.waft.inspector.screenshot","params":{"id":"e1","maxPixelRatio":2}}"#,
        )
        .expect("a request");
        assert_eq!(request.id, json!(7));
        assert_eq!(request.method, "ext.waft.inspector.screenshot");
        assert_eq!(request.params["maxPixelRatio"], "2");
        assert_eq!(Request::parse(&request.to_line()), Ok(request));
    }

    #[test]
    fn a_line_that_is_not_a_request_is_answered_with_an_error() {
        let response = Request::parse("{").expect_err("not JSON");
        assert_eq!(response.outcome.expect_err("an error").code, -32700);
        let response = Request::parse(r#"{"id":1}"#).expect_err("no method");
        assert_eq!(response.id, json!(1));
    }

    #[test]
    fn a_response_reads_back_as_written() {
        let answered = Response::result(json!(1), json!({ "data": "tree" }));
        assert_eq!(Response::parse(&answered.to_line()), Some(answered));
        let refused = Response::error(
            json!(2),
            RpcError::new(METHOD_NOT_FOUND, "no such extension".to_owned())
                .with_data(json!({ "registered": ["ext.waft.debugDumpApp"] })),
        );
        assert_eq!(Response::parse(&refused.to_line()), Some(refused));
    }
}
