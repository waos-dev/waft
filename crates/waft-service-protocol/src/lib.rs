//! How a client reaches the service extensions of a running Waft app: where apps are found, and
//! what a client and an app say to each other.
//!
//! Flutter's tools reach an app through the Dart VM service: JSON-RPC 2.0 with an extension's
//! name as the method, behind a token. This is the same in three parts, kept apart so each can
//! change alone:
//!
//! - **Messages** ([`Message`]): a client's hello carrying the app's token, then JSON-RPC 2.0
//!   requests, each answered by one response in the order asked, with Dart's error codes. They
//!   are JSON text, whatever carries them.
//! - **Transports** ([`Connection`]): what carries one message's text at a time. [`TcpLines`]
//!   carries them one to a line over TCP, for an app on this machine; [`ChildLines`] over a
//!   relay command's stdin and stdout, for a platform that carries them its own way, as WaOS
//!   does for its programs.
//! - **Listings and discovery** ([`ListedApp`], [`Discovery`]): a running app lists the one
//!   address that reaches it, with what names it, and sources of listings are merged into one
//!   list. [`ListingFolder`] reads this machine's folder, [`listing_dir`], which only its user
//!   can read; an app removes its listing when it ends.

mod connection;
mod discovery;
mod listing;
mod message;

pub use connection::{ChildLines, Client, Connection, TcpLines};
pub use discovery::{Discovery, ListingFolder, discover};
pub use listing::{Address, ListedApp, listing_dir};
pub use message::{
    EXTENSION_ERROR, INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND, Message, PARSE_ERROR,
    Request, Response, RpcError,
};
