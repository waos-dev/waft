//! `ext.waft.driver`: flutter_driver's commands, which a tool sends a running app to act as its
//! user would: find widgets, tap, press, type, scroll, wait, read text and picture a view.
//! Registered in every debug build, where flutter_driver's need the app to call
//! `enableFlutterDriverExtension` first; it changes nothing until a command arrives.
//!
//! A command names its widget by a CSS selector (`selector=…`, see [`selector`]) or by the id
//! `cargo waft ext tree` printed (`id=inspector-…`), for this run of the app ([`target`]).
//!
//! A command that acts first waits until the app has settled, flutter_driver's frame sync, and
//! fails when it does not within five seconds, as on a screen with a spinner; `frameSync=false`
//! acts without waiting, flutter_driver's `runUnsynchronized`. It then waits for its widget, as
//! flutter_driver's `waitForElement`, and a touch acts only on a widget a touch would reach
//! (`hitTestable`). Having acted, it waits again, so its answer comes once what it did shows,
//! and says whether the app settled then.

mod commands;
mod controller;
mod selector;
mod target;

use std::rc::Rc;

use waft_embedder::{ServiceExtensionError, ServiceExtensionParams, ServiceExtensionResult};
use waft_foundation::{App, ServiceExtensions, Task};

/// The commands, as `command=` names them.
const COMMANDS: &str = "get_health, find, tap, long_press, enter_text, send_text_input_action, \
                        scroll, scroll_into_view, wait_for, get_text, screenshot";

/// Registers `ext.waft.driver`, beside the inspector's extensions.
pub(crate) fn init_service_extensions(app: &mut App) {
    ServiceExtensions::register(app, "ext.waft.driver", Rc::new(driver_command));
}

/// Runs the command `command=` names.
fn driver_command(app: &mut App, params: ServiceExtensionParams) -> Task<ServiceExtensionResult> {
    let command = match params.require("command") {
        Ok(command) => command.to_owned(),
        Err(error) => return Task::ready(Err(error)),
    };
    match command.as_str() {
        "get_health" => Task::ready(commands::get_health()),
        "find" => Task::ready(commands::find(app, &params)),
        "screenshot" => commands::screenshot(app, &params),
        "tap" => app.spawn(async move |cx| commands::tap(cx, &params).await),
        "long_press" => app.spawn(async move |cx| commands::long_press(cx, &params).await),
        "enter_text" => app.spawn(async move |cx| commands::enter_text(cx, &params).await),
        "send_text_input_action" => {
            app.spawn(async move |cx| commands::send_text_input_action(cx, &params).await)
        }
        "scroll" => app.spawn(async move |cx| commands::scroll(cx, &params).await),
        "scroll_into_view" => {
            app.spawn(async move |cx| commands::scroll_into_view(cx, &params).await)
        }
        "wait_for" => app.spawn(async move |cx| commands::wait_for(cx, &params).await),
        "get_text" => app.spawn(async move |cx| commands::get_text(cx, &params).await),
        other => Task::ready(Err(ServiceExtensionError::InvalidParams(format!(
            "`{other}` is no driver command; the commands are {COMMANDS}"
        )))),
    }
}

#[cfg(test)]
mod tests;
