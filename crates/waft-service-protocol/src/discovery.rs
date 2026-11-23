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

/// Every app the sources find, each once: an app is its device and process.
pub fn discover(sources: &[&dyn Discovery]) -> Vec<ListedApp> {
    let mut apps: Vec<ListedApp> = Vec::new();
    for app in sources.iter().flat_map(|source| source.apps()) {
        if !apps
            .iter()
            .any(|known| known.device == app.device && known.pid == app.pid)
        {
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

    #[test]
    fn an_app_two_sources_find_is_listed_once() {
        let address = Address::Tcp("127.0.0.1:9000".parse().unwrap());
        let notes = ListedApp::new("notes", None, 42, address);
        let waos = ListedApp::new("waos", None, 43, address);
        let both = discover(&[
            &Fixed(vec![notes.clone()]),
            &Fixed(vec![notes.clone(), waos.clone()]),
        ]);
        assert_eq!(both, [notes, waos]);
    }
}
