//! How messages travel: a transport moves one message's text at a time and knows nothing of what
//! it says. `TcpLines` is the transport of an app on this machine; a WebSocket will be the web's.

use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};

use serde_json::{Value, json};

use crate::listing::{Address, ListedApp};
use crate::message::{Message, Request, RpcError};

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

/// A client's connection to one running app, by the transport its listing names.
pub struct Client {
    connection: Box<dyn Connection>,
    next_id: u64,
}

impl Client {
    /// Connects to `app` and says hello. An app that ended since it listed itself refuses the
    /// connection.
    pub fn connect(app: &ListedApp) -> io::Result<Client> {
        let mut connection: Box<dyn Connection> = match app.address {
            Address::Tcp(address) => Box::new(TcpLines::connect(address)?),
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
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the app's answer is not a JSON-RPC response",
            )),
        }
    }
}
