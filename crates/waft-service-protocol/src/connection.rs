//! How messages travel: a transport moves one message's text at a time and knows nothing of what
//! it says. `TcpLines` is the transport of an app on this machine; a WebSocket will be the web's.

use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

use crate::listing::{Address, ListedApp};
use crate::message::{Message, Request, Response, RpcError};

/// One connection, over whatever transport: each message's text, in order.
pub trait Connection {
    fn send(&mut self, text: &str) -> io::Result<()>;

    /// The next message's text, or `None` once the other side has closed the connection.
    fn receive(&mut self) -> io::Result<Option<String>>;
}

/// Messages over TCP, one to a line.
pub struct TcpLines {
    stream: BufReader<TcpStream>,
}

impl TcpLines {
    pub fn new(stream: TcpStream) -> TcpLines {
        TcpLines {
            stream: BufReader::new(stream),
        }
    }

    pub fn connect(address: SocketAddr) -> io::Result<TcpLines> {
        TcpStream::connect(address).map(TcpLines::new)
    }
}

impl Connection for TcpLines {
    fn send(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.stream.get_mut(), "{text}")
    }

    fn receive(&mut self) -> io::Result<Option<String>> {
        let mut line = String::new();
        match self.stream.read_line(&mut line)? {
            0 => Ok(None),
            _ => Ok(Some(line.trim_end_matches(['\r', '\n']).to_owned())),
        }
    }
}

/// Messages one to a line over a relay's stdin and stdout: the command a listing's
/// [`Address::Command`] names, run for as long as the connection is held, as ssh runs its
/// `ProxyCommand`. Dropping the connection closes the relay's stdin, which ends it.
pub struct ChildLines {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl ChildLines {
    /// Runs `arguments` (the command, then its own arguments) as a relay. What it writes to its
    /// stderr reaches this process's, so a relay's own errors are seen.
    pub fn spawn(arguments: &[String]) -> io::Result<ChildLines> {
        let (program, rest) = arguments.split_first().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "a relay names a command")
        })?;
        let mut child = Command::new(program)
            .args(rest)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().expect("the relay's stdout is piped"));
        Ok(ChildLines {
            child,
            stdin,
            stdout,
        })
    }
}

impl Connection for ChildLines {
    fn send(&mut self, text: &str) -> io::Result<()> {
        let stdin = self.stdin.as_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "the relay's stdin is closed")
        })?;
        writeln!(stdin, "{text}")?;
        stdin.flush()
    }

    fn receive(&mut self) -> io::Result<Option<String>> {
        let mut line = String::new();
        match self.stdout.read_line(&mut line)? {
            0 => Ok(None),
            _ => Ok(Some(line.trim_end_matches(['\r', '\n']).to_owned())),
        }
    }
}

impl Drop for ChildLines {
    fn drop(&mut self) {
        // The relay ends once its stdin closes.
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

/// A client's connection to one running app, by the transport its listing names.
pub struct Client {
    connection: Box<dyn Connection>,
    next_id: u64,
}

impl Client {
    /// Connects to `app` and says hello. An app that ended since it listed itself refuses the
    /// connection.
    pub fn connect(app: &ListedApp) -> io::Result<Client> {
        let mut connection: Box<dyn Connection> = match &app.address {
            Address::Tcp(address) => Box::new(TcpLines::connect(*address)?),
            Address::Command(arguments) => Box::new(ChildLines::spawn(arguments)?),
        };
        let hello = Message::Hello {
            token: app.token.clone(),
        };
        connection.send(&hello.to_text())?;
        Ok(Client {
            connection,
            next_id: 1,
        })
    }

    /// Calls `method` with `params` and waits for its answer.
    pub fn call(
        &mut self,
        method: &str,
        params: BTreeMap<String, String>,
    ) -> io::Result<Result<Value, RpcError>> {
        let request = Message::Request(Request {
            id: json!(self.next_id),
            method: method.to_owned(),
            params,
        });
        self.next_id += 1;
        self.connection.send(&request.to_text())?;
        let text = self.connection.receive()?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the app closed the connection",
            )
        })?;
        match Message::from_text(&text) {
            Ok(Message::Response(response)) => Ok(response.outcome),
            Err(Response {
                outcome: Err(why), ..
            }) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("the app's answer cannot be read: {}", why.message),
            )),
            Ok(_) | Err(_) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the app's answer is not a JSON-RPC response",
            )),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A relay carries each message's text both ways: `cat` sends each line back.
    #[test]
    fn a_relay_carries_each_line_both_ways() {
        let mut relay = ChildLines::spawn(&["cat".to_owned()]).expect("cat runs");
        relay.send(r#"{"token":""}"#).expect("sent");
        relay
            .send(r#"{"jsonrpc":"2.0","id":1,"method":"ext.waft.debugDumpApp"}"#)
            .expect("sent");
        assert_eq!(
            relay.receive().expect("read").as_deref(),
            Some(r#"{"token":""}"#)
        );
        assert_eq!(
            relay.receive().expect("read").as_deref(),
            Some(r#"{"jsonrpc":"2.0","id":1,"method":"ext.waft.debugDumpApp"}"#)
        );
    }
}
