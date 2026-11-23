//! A running app as it lists itself: what names it, and the one address that reaches it.

use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde_json::{Value, json};

/// The folder apps on this machine list themselves in, readable by the user alone: the user's
/// runtime directory where the system gives one (`$XDG_RUNTIME_DIR`), else the user's temporary
/// folder, which is the user's own on macOS and Windows and is named by the user's id elsewhere.
/// On a phone, an app lists itself in its own temporary folder, `waft/` under it.
pub fn listing_dir() -> PathBuf {
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime).join("waft");
    }
    std::env::temp_dir().join(per_user_name())
}

/// The folder's name under a temporary folder others may share.
#[cfg(all(unix, not(target_os = "macos")))]
fn per_user_name() -> String {
    use std::os::unix::fs::MetadataExt;
    // The owner of the user's home is the user, with no call into the C library.
    let uid = std::env::var_os("HOME")
        .and_then(|home| std::fs::metadata(home).ok())
        .map(|home| home.uid());
    match uid {
        Some(uid) => format!("waft-{uid}"),
        None => "waft".to_owned(),
    }
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn per_user_name() -> String {
    "waft".to_owned()
}

/// The one way to reach a running app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Address {
    /// Messages one to a line over TCP (`TcpLines`), on this machine's loopback.
    Tcp(SocketAddr),
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Address::Tcp(address) => write!(f, "tcp://{address}"),
        }
    }
}

impl FromStr for Address {
    type Err = String;

    fn from_str(text: &str) -> Result<Address, String> {
        match text.split_once("://") {
            Some(("tcp", address)) => address
                .parse()
                .map(Address::Tcp)
                .map_err(|error| format!("`{text}`: {error}")),
            _ => Err(format!("`{text}` names no transport this client speaks")),
        }
    }
}

/// A running app, as it lists itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedApp {
    /// The app's executable's name, as `waos`.
    pub app: String,
    /// The app's identifier, as `dev.waos.launcher`, where its host knows one: a bundle's on
    /// macOS and iOS, a package's on Android, the app id an app gives on Linux.
    pub identifier: Option<String>,
    /// The device it runs on, as `cargo waft devices` names it, or `None` for this machine.
    pub device: Option<String>,
    pub pid: u32,
    pub address: Address,
    /// What a client's hello must carry.
    pub token: String,
}

impl ListedApp {
    /// The app `app`, process `pid` on this machine, reached at `address`, with a new token.
    pub fn new(
        app: impl Into<String>,
        identifier: Option<String>,
        pid: u32,
        address: Address,
    ) -> ListedApp {
        ListedApp {
            app: app.into(),
            identifier,
            device: None,
            pid,
            address,
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

    /// The file this app is listed in, in `dir`.
    pub fn file_in(&self, dir: &Path) -> PathBuf {
        dir.join(format!("{}-{}.json", self.app, self.pid))
    }

    /// Lists this app in `dir`, readable by the user alone, and answers the file written, which
    /// the app removes when it ends.
    pub fn write_in(&self, dir: &Path) -> io::Result<PathBuf> {
        create_private_dir(dir)?;
        let path = self.file_in(dir);
        let mut file = private_file(&path)?;
        file.write_all(self.to_json().to_string().as_bytes())?;
        Ok(path)
    }

    fn to_json(&self) -> Value {
        json!({
            "app": self.app,
            "identifier": self.identifier,
            "device": self.device,
            "pid": self.pid,
            "address": self.address.to_string(),
            "token": self.token,
        })
    }

    fn from_json(text: &str) -> Option<ListedApp> {
        let value: Value = serde_json::from_str(text).ok()?;
        let text_of = |name| value.get(name).and_then(Value::as_str).map(str::to_owned);
        Some(ListedApp {
            app: text_of("app")?,
            identifier: text_of("identifier"),
            device: text_of("device"),
            pid: u32::try_from(value.get("pid")?.as_u64()?).ok()?,
            address: value.get("address")?.as_str()?.parse().ok()?,
            token: text_of("token")?,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_reads_back_as_written() {
        let dir = std::env::temp_dir().join(format!("waft-listing-test-{}", std::process::id()));
        let address = Address::Tcp("127.0.0.1:9000".parse().unwrap());
        let app = ListedApp::new("notes", Some("dev.example.notes".to_owned()), 42, address);
        let path = app.write_in(&dir).expect("written");
        assert_eq!(ListedApp::all_in(&dir), std::slice::from_ref(&app));
        std::fs::remove_file(path).expect("removed");
        std::fs::remove_dir(&dir).expect("removed");
    }

    #[test]
    fn an_address_names_its_transport() {
        let address = Address::Tcp("127.0.0.1:52341".parse().unwrap());
        assert_eq!(address.to_string(), "tcp://127.0.0.1:52341");
        assert_eq!("tcp://127.0.0.1:52341".parse(), Ok(address));
        assert!("ws://127.0.0.1:8080/ext".parse::<Address>().is_err());
    }

    #[test]
    fn a_token_is_128_bits_of_hex() {
        let token = new_token();
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|character| character.is_ascii_hexdigit()));
    }
}
