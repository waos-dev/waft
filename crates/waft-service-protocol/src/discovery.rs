//! Where running apps are found: one source for each place apps can be, merged into one list.

use std::path::PathBuf;

use crate::listing::{ListedApp, listing_dir};

/// A source of running apps.
pub trait Discovery {
    fn apps(&self) -> Vec<ListedApp>;
}

/// The apps on this machine: those listed in [`listing_dir`].
pub struct ListingFolder {
    dir: PathBuf,
}

impl ListingFolder {
    /// This machine's listing folder.
    pub fn new() -> ListingFolder {
        ListingFolder { dir: listing_dir() }
    }

    /// The folder it reads, where a stale listing is removed.
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }
}

impl Default for ListingFolder {
    fn default() -> ListingFolder {
        ListingFolder::new()
    }
}

impl Discovery for ListingFolder {
    fn apps(&self) -> Vec<ListedApp> {
        ListedApp::all_in(&self.dir)
    }
}

/// Every app the sources find, each once: an app is the one address that reaches it, since apps
/// a platform runs share its process (a WaOS program's is WaOS's).
pub fn discover(sources: &[&dyn Discovery]) -> Vec<ListedApp> {
    let mut apps: Vec<ListedApp> = Vec::new();
    for app in sources.iter().flat_map(|source| source.apps()) {
        if !apps.iter().any(|known| known.address == app.address) {
            apps.push(app);
        }
    }
    apps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listing::Address;

    struct Fixed(Vec<ListedApp>);

    impl Discovery for Fixed {
        fn apps(&self) -> Vec<ListedApp> {
            self.0.clone()
        }
    }

    fn relay(id: &str) -> Address {
        Address::Command(
            ["waos", "ext", "--id", id, "--stdio"]
                .map(String::from)
                .to_vec(),
        )
    }

    /// An app two sources find is listed once, and two programs a platform runs in its one
    /// process are two apps.
    #[test]
    fn an_app_is_the_address_that_reaches_it() {
        let notes = ListedApp::new(
            "notes",
            None,
            42,
            Address::Tcp("127.0.0.1:9000".parse().unwrap()),
        );
        let clock = ListedApp::new("clock", None, 7, relay("dev.example.clock"));
        let timer = ListedApp::new("timer", None, 7, relay("dev.example.timer"));
        let found = discover(&[
            &Fixed(vec![notes.clone()]),
            &Fixed(vec![notes.clone(), clock.clone(), timer.clone()]),
        ]);
        assert_eq!(found, [notes, clock, timer]);
    }
}
