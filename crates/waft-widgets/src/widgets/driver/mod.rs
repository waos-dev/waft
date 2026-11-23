//! `ext.waft.driver`: flutter_driver's commands, which a tool sends a running app to act as its
//! user would: find widgets, tap, type, scroll, wait and read text. Registered in every debug
//! build, where flutter_driver's need the app to call `enableFlutterDriverExtension` first; it
//! changes nothing until a command arrives.
//!
//! A command names its widget by a CSS selector (`selector=…`, see [`selector`]) or by the id
//! `cargo waft ext tree` printed (`id=inspector-…`), for this run of the app.

mod controller;
mod selector;

use std::rc::Rc;

use serde_json::{Value, json};
use waft_embedder::{ServiceExtensionError, ServiceExtensionParams, ServiceExtensionResult};
use waft_foundation::{App, AsyncApp, DiagnosticsNode, ServiceExtensions, Task};

use self::selector::WidgetSelector;
use crate::widgets::widget_inspector::{
    InspectorSerializationDelegate, element_named, from_inspector_id,
};
use crate::{AnyElement, WidgetsBinding};

/// Registers `ext.waft.driver`, beside the inspector's extensions.
pub(crate) fn init_service_extensions(app: &mut App) {
    ServiceExtensions::register(app, "ext.waft.driver", Rc::new(driver_command));
}

/// The commands, as `command=` names them.
const COMMANDS: &str = "find, tap";

/// Runs the command `command=` names.
fn driver_command(app: &mut App, params: ServiceExtensionParams) -> Task<ServiceExtensionResult> {
    match params.require("command") {
        Ok("find") => Task::ready(find(app, &params)),
        Ok("tap") => app.spawn(async move |cx| tap(cx, &params).await),
        Ok(other) => Task::ready(Err(ServiceExtensionError::InvalidParams(format!(
            "`{other}` is no driver command; the commands are {COMMANDS}"
        )))),
        Err(error) => Task::ready(Err(error)),
    }
}

/// `find`: every widget the command names, each as the inspector's node of it, as
/// `{"found": [node, …]}`.
fn find(app: &mut App, params: &ServiceExtensionParams) -> ServiceExtensionResult {
    let found: Vec<Value> = named(app, params)?
        .into_iter()
        .map(|element| node_of(app, element))
        .collect();
    Ok(json!({ "found": found }))
}

/// `tap`: a tap at the centre of the one widget the command names, once the app has settled,
/// answered once it has settled again, as `{"tapped": node, "settled": bool}`; `settled` is
/// false where the app kept drawing and the tap went ahead regardless.
async fn tap(cx: &mut AsyncApp, params: &ServiceExtensionParams) -> ServiceExtensionResult {
    let settled_before = controller::settle(cx).await;
    let tapped = cx.update(|app| {
        let element = the_one(app, params)?;
        let (at, view) = controller::center_of(app, element).ok_or_else(|| {
            ServiceExtensionError::Failed(format!(
                "{} renders no box to tap",
                line_of(app, element)
            ))
        })?;
        controller::tap_at(app, at, view);
        Ok::<_, ServiceExtensionError>(node_of(app, element))
    })?;
    let settled_after = controller::settle(cx).await;
    Ok(json!({ "tapped": tapped, "settled": settled_before && settled_after }))
}

/// The one widget the command names; where it names none or several, why, with each widget's
/// line to choose one by.
fn the_one(
    app: &mut App,
    params: &ServiceExtensionParams,
) -> Result<AnyElement, ServiceExtensionError> {
    let mut named = named(app, params)?;
    let naming = params
        .get("selector")
        .or(params.get("id"))
        .unwrap_or_default();
    match named.len() {
        1 => Ok(named.remove(0)),
        0 => Err(ServiceExtensionError::Failed(format!(
            "`{naming}` names no widget now"
        ))),
        count => {
            let lines: Vec<String> = named.iter().map(|element| line_of(app, *element)).collect();
            Err(ServiceExtensionError::Failed(format!(
                "`{naming}` names {count} widgets; name one by a narrower selector or its id:\n  {}",
                lines.join("\n  ")
            )))
        }
    }
}

/// The onstage widgets `selector=` matches, or the one `id=` names, in the tree's order.
fn named(
    app: &mut App,
    params: &ServiceExtensionParams,
) -> Result<Vec<AnyElement>, ServiceExtensionError> {
    let Some(root) = WidgetsBinding::instance(app).root_element(app) else {
        return Ok(Vec::new());
    };
    match (params.get("selector"), params.get("id")) {
        (Some(css), None) => {
            let selector =
                WidgetSelector::parse(css).map_err(ServiceExtensionError::InvalidParams)?;
            Ok(selector.select(app, root))
        }
        (None, Some(id)) => Ok(from_inspector_id(id)
            .and_then(|id| element_named(app, root, id))
            .into_iter()
            .collect()),
        _ => Err(ServiceExtensionError::InvalidParams(
            "a command names its widget by `selector` or by `id`, one of them".to_owned(),
        )),
    }
}

/// `element` as one line, as `cargo waft ext tree` prints it: its description, the line that
/// made it and its id.
fn line_of(app: &App, element: AnyElement) -> String {
    let node = node_of(app, element);
    let mut line = node["description"].as_str().unwrap_or("?").to_owned();
    if let Some(location) = node["creationLocation"].as_object() {
        let file = location.get("file").and_then(Value::as_str).unwrap_or("?");
        let at = location.get("line").and_then(Value::as_u64).unwrap_or(0);
        line.push_str(&format!("  {file}:{at}"));
    }
    if let Some(id) = node["valueId"].as_str() {
        line.push_str(&format!("  {id}"));
    }
    line
}

/// `element` as the inspector's node of it, alone: its description, type, the line that made
/// it and its id.
fn node_of(app: &App, element: AnyElement) -> Value {
    let delegate = InspectorSerializationDelegate {
        group_name: Some("driver".to_owned()),
        subtree_depth: 0,
        ..InspectorSerializationDelegate::default()
    };
    Value::Object(
        element
            .to_diagnostics_node(app, None)
            .to_json_map(&delegate),
    )
}

#[cfg(test)]
mod tests;
