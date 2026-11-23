//! The app's service extensions, served on this machine in a debug build, as Flutter's VM service
//! serves a debug app's: the host listens on a loopback port and lists the app where
//! `waft ext` finds it (`waft-service-protocol`), and hands each call to the loop, which hands it
//! to the framework (`EmbedderClient::service_extension_call`).
//!
//! A thread accepts connections, and one per connection reads its requests and waits for each
//! answer before it reads the next, so a connection's answers keep the order of its requests.
//! The loop hears when the first connection arrives and when the last one leaves: while a tool
//! is connected, the app is driven, as a browser Playwright drives is.
//! The listing is removed when the loop ends; one left by a process that was killed is passed
//! over by a client, which cannot connect to it.

use std::collections::BTreeMap;
use std::io;
use std::net::{Ipv4Addr, TcpListener};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};

use serde_json::json;
use waft_embedder::{ServiceExtensionError, ServiceExtensionResult};
use waft_service_protocol::{
    Address, Connection, EXTENSION_ERROR, INVALID_PARAMS, INVALID_REQUEST, ListedApp,
    METHOD_NOT_FOUND, Message, Request, Response, RpcError, TcpLines, listing_dir,
};

use crate::main_loop::{MainLoopEvent, MainLoopProxy};

/// A call a connection sent the loop: what to call, and where its answer goes.
pub(crate) struct IncomingCall {
    pub(crate) name: String,
    pub(crate) params: BTreeMap<String, String>,
    pub(crate) answer: Sender<ServiceExtensionResult>,
}

/// Serving while it lives: dropping it removes the app's listing.
pub(crate) struct ServiceExtensionServer {
    listing: PathBuf,
}

impl ServiceExtensionServer {
    /// Starts serving through `proxy`'s loop, in a debug build, listing the app with its
    /// `identifier` where it has one. Where this machine lets the app
    /// neither listen nor list itself, it goes without, as an app with no VM service would.
    pub(crate) fn start(
        proxy: MainLoopProxy,
        identifier: Option<String>,
    ) -> Option<ServiceExtensionServer> {
        if !cfg!(debug_assertions) {
            return None;
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?;
        let address = Address::Tcp(listener.local_addr().ok()?);
        let app = ListedApp::new(app_name(), identifier, std::process::id(), address);
        let listing = app.write_in(&listing_dir()).ok()?;
        let to_the_loop = move |event| {
            proxy.send(match event {
                ServerEvent::Call(call) => MainLoopEvent::ServiceExtensionCall(call),
                ServerEvent::ToolsConnected(connected) => MainLoopEvent::ToolsConnected(connected),
            });
        };
        listen(listener, app.token, to_the_loop).ok()?;
        Some(ServiceExtensionServer { listing })
    }
}

impl Drop for ServiceExtensionServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.listing);
    }
}

/// The name a client finds the app by: its executable's.
fn app_name() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "app".to_owned())
}

/// What the server tells the loop.
enum ServerEvent {
    /// A connection's call, for the loop to hand to the framework.
    Call(IncomingCall),
    /// Whether any tool is connected now: the first has arrived, or the last has left.
    ToolsConnected(bool),
}

/// Where the server's events go: the loop.
trait Post: Fn(ServerEvent) + Clone + Send + 'static {}

impl<P: Fn(ServerEvent) + Clone + Send + 'static> Post for P {}

/// The connections that said a hello with the app's token, counted so the loop hears when the
/// first arrives and when the last leaves.
#[derive(Clone, Default)]
struct Tools(Arc<AtomicUsize>);

impl Tools {
    /// One more connection, until the returned guard is dropped.
    fn arrived(&self, post: &impl Post) -> ToolLeaves {
        if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            post(ServerEvent::ToolsConnected(true));
        }
        ToolLeaves {
            tools: self.clone(),
            post: Box::new(post.clone()),
        }
    }
}

/// A connection, until it closes.
struct ToolLeaves {
    tools: Tools,
    post: Box<dyn Fn(ServerEvent) + Send>,
}

impl Drop for ToolLeaves {
    fn drop(&mut self) {
        if self.tools.0.fetch_sub(1, Ordering::SeqCst) == 1 {
            (self.post)(ServerEvent::ToolsConnected(false));
        }
    }
}

/// Accepts connections on a thread of its own, until the process ends, and serves each on
/// another.
fn listen(listener: TcpListener, token: String, post: impl Post) -> io::Result<()> {
    #[expect(
        clippy::disallowed_methods,
        reason = "a platform crate starts the threads its transport needs"
    )]
    std::thread::Builder::new()
        .name("waft service extensions".to_owned())
        .spawn(move || {
            let tools = Tools::default();
            for stream in listener.incoming().filter_map(Result::ok) {
                let (token, post, tools) = (token.clone(), post.clone(), tools.clone());
                #[expect(
                    clippy::disallowed_methods,
                    reason = "a platform crate starts the threads its transport needs"
                )]
                let _ = std::thread::Builder::new()
                    .name("waft service extension connection".to_owned())
                    .spawn(move || {
                        let _ = serve(TcpLines::new(stream), &token, &post, &tools);
                    });
            }
        })
        .map(drop)
}

/// Reads one connection's requests after its hello, and answers each in turn.
fn serve(
    mut connection: impl Connection,
    token: &str,
    post: &impl Post,
    tools: &Tools,
) -> io::Result<()> {
    let hello = connection.receive()?.map(|text| Message::from_text(&text));
    match hello {
        Some(Ok(Message::Hello { token: sent })) if sent == token => {}
        _ => return Ok(()),
    }
    let _connected = tools.arrived(post);
    while let Some(text) = connection.receive()? {
        let response = match Message::from_text(&text) {
            Ok(Message::Request(request)) => answer(request, post),
            Ok(_) => Response::error(
                serde_json::Value::Null,
                RpcError::new(INVALID_REQUEST, "an app answers requests alone".to_owned()),
            ),
            Err(response) => response,
        };
        connection.send(&Message::Response(response).to_text())?;
    }
    Ok(())
}

/// Hands `request` to the loop and waits for the framework's answer.
fn answer(request: Request, post: &impl Post) -> Response {
    let (answer, answered) = mpsc::channel();
    post(ServerEvent::Call(IncomingCall {
        name: request.method,
        params: request.params,
        answer,
    }));
    let result = answered.recv().unwrap_or_else(|_| {
        Err(ServiceExtensionError::Failed(
            "the app ended before it answered".to_owned(),
        ))
    });
    match result {
        Ok(value) => Response::result(request.id, value),
        Err(error) => Response::error(request.id, rpc_error(error)),
    }
}

/// A framework error as the JSON-RPC error Dart's VM service answers with.
fn rpc_error(error: ServiceExtensionError) -> RpcError {
    let message = error.to_string();
    match error {
        ServiceExtensionError::Unknown { registered, .. } => {
            RpcError::new(METHOD_NOT_FOUND, message).with_data(json!({ "registered": registered }))
        }
        ServiceExtensionError::InvalidParams(_) => RpcError::new(INVALID_PARAMS, message),
        ServiceExtensionError::Failed(_) => RpcError::new(EXTENSION_ERROR, message),
    }
}

/// Where the framework's answer to a call goes: back to the connection waiting for it.
pub(crate) fn reply(
    answer: Sender<ServiceExtensionResult>,
) -> waft_embedder::ServiceExtensionReply {
    waft_embedder::ServiceExtensionReply::new(move |result| {
        let _ = answer.send(result);
    })
}

#[cfg(test)]
mod tests {
    use waft_service_protocol::Client;

    use super::*;

    /// A server whose calls are answered on the spot, with the extension's name and parameters
    /// sent back, or as unknown for `ext.test.missing`; and how a client finds it.
    fn echoing_server() -> ListedApp {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a port");
        let address = Address::Tcp(listener.local_addr().unwrap());
        let app = ListedApp::new("echo", None, std::process::id(), address);
        let echo = |event: ServerEvent| {
            let ServerEvent::Call(call) = event else {
                return;
            };
            let result = if call.name == "ext.test.missing" {
                Err(ServiceExtensionError::Unknown {
                    name: call.name,
                    registered: vec!["ext.test.echo".to_owned()],
                })
            } else {
                Ok(json!({ "name": call.name, "params": call.params }))
            };
            let _ = call.answer.send(result);
        };
        listen(listener, app.token.clone(), echo).expect("listening");
        app
    }

    /// A client that says hello calls an extension and reads its answer, and the next one's,
    /// over the one connection.
    #[test]
    fn a_client_calls_through_the_socket_and_reads_each_answer() {
        let app = echoing_server();
        let mut client = Client::connect(&app).expect("connected");
        let params = BTreeMap::from([("id".to_owned(), "e1".to_owned())]);
        assert_eq!(
            client.call("ext.test.echo", params).expect("answered"),
            Ok(json!({ "name": "ext.test.echo", "params": { "id": "e1" } }))
        );
        let refused = client
            .call("ext.test.missing", BTreeMap::new())
            .expect("answered")
            .expect_err("unknown");
        assert_eq!(refused.code, METHOD_NOT_FOUND);
    }

    /// A connection whose first line lacks the token is closed before it can call anything.
    #[test]
    fn a_client_without_the_token_is_turned_away() {
        let app = echoing_server();
        let stranger = ListedApp {
            token: "guess".to_owned(),
            ..app
        };
        let mut client = Client::connect(&stranger).expect("connected");
        assert!(client.call("ext.test.echo", BTreeMap::new()).is_err());
    }

    /// The loop hears when the first tool arrives and when the last leaves, and nothing of a
    /// connection without the token.
    #[test]
    fn the_loop_hears_when_tools_come_and_go() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a port");
        let address = Address::Tcp(listener.local_addr().unwrap());
        let app = ListedApp::new("echo", None, std::process::id(), address);
        let (heard, hearing) = mpsc::channel();
        let report = move |event: ServerEvent| match event {
            ServerEvent::ToolsConnected(connected) => {
                let _ = heard.send(connected);
            }
            ServerEvent::Call(call) => {
                let _ = call.answer.send(Ok(json!({})));
            }
        };
        listen(listener, app.token.clone(), report).expect("listening");
        let wait = std::time::Duration::from_secs(5);

        let stranger = ListedApp {
            token: "guess".to_owned(),
            ..app.clone()
        };
        let mut turned_away = Client::connect(&stranger).expect("connected");
        assert!(turned_away.call("ext.test.echo", BTreeMap::new()).is_err());

        let mut first = Client::connect(&app).expect("connected");
        first
            .call("ext.test.echo", BTreeMap::new())
            .expect("answered")
            .expect("ok");
        assert_eq!(
            hearing.recv_timeout(wait),
            Ok(true),
            "the first tool arrived"
        );
        let mut second = Client::connect(&app).expect("connected");
        second
            .call("ext.test.echo", BTreeMap::new())
            .expect("answered")
            .expect("ok");
        drop(first);
        drop(second);
        assert_eq!(hearing.recv_timeout(wait), Ok(false), "the last tool left");
        assert!(hearing.try_recv().is_err(), "nothing heard in between");
    }

    #[test]
    fn an_unknown_extension_is_answered_as_a_method_not_found_naming_the_registered() {
        let error = rpc_error(ServiceExtensionError::Unknown {
            name: "ext.waft.nothing".to_owned(),
            registered: vec!["ext.waft.debugDumpApp".to_owned()],
        });
        assert_eq!(error.code, METHOD_NOT_FOUND);
        assert_eq!(
            error.data,
            Some(json!({ "registered": ["ext.waft.debugDumpApp"] }))
        );
    }

    #[test]
    fn bad_params_and_failures_keep_darts_codes() {
        assert_eq!(
            rpc_error(ServiceExtensionError::InvalidParams(
                "`id` is required".to_owned()
            ))
            .code,
            INVALID_PARAMS
        );
        assert_eq!(
            rpc_error(ServiceExtensionError::Failed("no view".to_owned())).code,
            EXTENSION_ERROR
        );
    }
}
