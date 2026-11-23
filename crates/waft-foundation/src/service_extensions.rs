//! The service extensions this app serves: Dart's `BindingBase.registerServiceExtension`, and
//! the table of them the Dart VM keeps (`dart:developer`'s `registerExtension`).
//!
//! A binding registers its extensions when it is set up, as Dart's `initServiceExtensions` does,
//! and only in a debug build, as Dart's are only in a debug or profile one. A host carries a call
//! here from a client ([`ServiceExtensions::call`]) and the answer back; the table itself does
//! no I/O. `ext.waft.listServiceExtensions` answers the names registered, which Flutter's VM
//! answers as an isolate's `extensionRPCs`.

use std::collections::BTreeMap;
use std::rc::Rc;

use waft_embedder::{ServiceExtensionError, ServiceExtensionParams, ServiceExtensionResult};

use crate::{App, Task};

/// What answers one service extension: Dart's `ServiceExtensionCallback`, given the app and the
/// call's parameters, answering when its task does.
pub type ServiceExtensionCallback =
    Rc<dyn Fn(&mut App, ServiceExtensionParams) -> Task<ServiceExtensionResult>>;

/// The extension that lists the others.
const LIST_SERVICE_EXTENSIONS: &str = "ext.waft.listServiceExtensions";

/// The service extensions registered, by name.
#[derive(Default)]
pub struct ServiceExtensions {
    callbacks: BTreeMap<String, ServiceExtensionCallback>,
}

impl ServiceExtensions {
    /// Registers `callback` to answer calls to `name`: Dart's `registerServiceExtension`, with
    /// the whole method name, as `dart:developer` takes it (`ext.waft.debugDumpApp`).
    ///
    /// # Panics
    ///
    /// Where `name` does not begin `ext.`, or is registered already, as `dart:developer` throws.
    pub fn register(app: &mut App, name: impl Into<String>, callback: ServiceExtensionCallback) {
        let name = name.into();
        assert!(
            name.starts_with("ext."),
            "a service extension's name begins `ext.`: {name}"
        );
        let table = ServiceExtensions::table(app);
        let previous = app.get_mut(table).callbacks.insert(name.clone(), callback);
        assert!(previous.is_none(), "{name} is registered already");
    }

    /// Registers `name` to answer `{"data": text}` with what `dump` prints, as Flutter's dump
    /// extensions (`debugDumpApp`, `debugDumpRenderTree`, …) answer.
    pub fn register_dump(app: &mut App, name: impl Into<String>, dump: fn(&mut App) -> String) {
        let callback: ServiceExtensionCallback =
            Rc::new(move |app, _params| Task::ready(Ok(serde_json::json!({ "data": dump(app) }))));
        ServiceExtensions::register(app, name, callback);
    }

    /// Answers a call to `name` with `params`: the registered extension's answer, or
    /// [`ServiceExtensionError::Unknown`] naming those that are registered.
    pub fn call(
        app: &mut App,
        name: &str,
        params: ServiceExtensionParams,
    ) -> Task<ServiceExtensionResult> {
        let table = ServiceExtensions::table(app);
        match app.get(table).callbacks.get(name).cloned() {
            Some(callback) => callback(app, params),
            None => Task::ready(Err(ServiceExtensionError::Unknown {
                name: name.to_owned(),
                registered: ServiceExtensions::names(app),
            })),
        }
    }

    /// The names of the extensions registered, in order.
    pub fn names(app: &mut App) -> Vec<String> {
        let table = ServiceExtensions::table(app);
        app.get(table).callbacks.keys().cloned().collect()
    }

    /// The app's table, set up on first use with the extension that lists the others, in a
    /// debug build.
    fn table(app: &mut App) -> crate::Handle<ServiceExtensions> {
        let table: crate::Handle<ServiceExtensions> = app.singleton();
        if cfg!(debug_assertions) && app.get(table).callbacks.is_empty() {
            let list: ServiceExtensionCallback = Rc::new(|app, _params| {
                Task::ready(Ok(
                    serde_json::json!({ "extensions": ServiceExtensions::names(app) }),
                ))
            });
            app.get_mut(table)
                .callbacks
                .insert(LIST_SERVICE_EXTENSIONS.to_owned(), list);
        }
        table
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;

    use super::*;
    use crate::AppCell;

    fn answer(app: &mut App, name: &str) -> ServiceExtensionResult {
        answered(ServiceExtensions::call(
            app,
            name,
            ServiceExtensionParams::default(),
        ))
    }

    /// The answer of a task that has it already, as these extensions do.
    fn answered(mut task: Task<ServiceExtensionResult>) -> ServiceExtensionResult {
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        match std::pin::Pin::new(&mut task).poll(&mut cx) {
            std::task::Poll::Ready(result) => result,
            std::task::Poll::Pending => panic!("the extension answers at once"),
        }
    }

    #[test]
    fn a_registered_extension_answers_its_calls() {
        let cell = AppCell::new();
        let mut app = cell.borrow_mut();
        ServiceExtensions::register(
            &mut app,
            "ext.test.echo",
            Rc::new(|_app, params| {
                Task::ready(Ok(serde_json::json!({ "said": params.get("words") })))
            }),
        );
        let params = ServiceExtensionParams::new([("words".to_owned(), "hello".to_owned())]);
        assert_eq!(
            answered(ServiceExtensions::call(&mut app, "ext.test.echo", params)),
            Ok(serde_json::json!({ "said": "hello" }))
        );
    }

    /// The table lists what it serves, itself included, as an isolate's `extensionRPCs` do.
    #[test]
    fn the_table_lists_what_it_serves() {
        let cell = AppCell::new();
        let mut app = cell.borrow_mut();
        ServiceExtensions::register(
            &mut app,
            "ext.test.echo",
            Rc::new(|_app, _params| Task::ready(Ok(serde_json::Value::Null))),
        );
        assert_eq!(
            answer(&mut app, "ext.waft.listServiceExtensions"),
            Ok(serde_json::json!({
                "extensions": ["ext.test.echo", "ext.waft.listServiceExtensions"]
            }))
        );
    }

    /// A call to a name nothing registered says what the app serves instead.
    #[test]
    fn an_unknown_extension_names_those_that_are_registered() {
        let cell = AppCell::new();
        let mut app = cell.borrow_mut();
        assert_eq!(
            answer(&mut app, "ext.test.missing"),
            Err(ServiceExtensionError::Unknown {
                name: "ext.test.missing".to_owned(),
                registered: vec!["ext.waft.listServiceExtensions".to_owned()],
            })
        );
    }

    #[test]
    #[should_panic(expected = "is registered already")]
    fn a_name_is_registered_once() {
        let cell = AppCell::new();
        let mut app = cell.borrow_mut();
        let callback: ServiceExtensionCallback =
            Rc::new(|_app, _params| Task::ready(Ok(serde_json::Value::Null)));
        ServiceExtensions::register(&mut app, "ext.test.echo", callback.clone());
        ServiceExtensions::register(&mut app, "ext.test.echo", callback);
    }
}
