//! `ext.waft.driver`: flutter_driver's commands, which a tool sends a running app to act as its
//! user would: find widgets, tap, type, scroll, wait and read text. Registered in every debug
//! build, where flutter_driver's need the app to call `enableFlutterDriverExtension` first; it
//! changes nothing until a command arrives.
//!
//! A command names its widget by a CSS selector (`selector=…`, see [`selector`]) or by the id
//! `cargo waft ext tree` printed (`id=inspector-…`), for this run of the app.

mod selector;

use std::rc::Rc;

use serde_json::{Value, json};
use waft_embedder::{ServiceExtensionError, ServiceExtensionParams, ServiceExtensionResult};
use waft_foundation::{App, DiagnosticsNode, ServiceExtensions, Task};

use self::selector::WidgetSelector;
use crate::widgets::widget_inspector::{
    InspectorSerializationDelegate, element_named, from_inspector_id,
};
use crate::{AnyElement, WidgetsBinding};

/// Registers `ext.waft.driver`, beside the inspector's extensions.
pub(crate) fn init_service_extensions(app: &mut App) {
    ServiceExtensions::register(app, "ext.waft.driver", Rc::new(driver_command));
}

/// Runs the command `command=` names.
fn driver_command(app: &mut App, params: ServiceExtensionParams) -> Task<ServiceExtensionResult> {
    let answer = match params.require("command") {
        Ok("find") => find(app, &params),
        Ok(other) => Err(ServiceExtensionError::InvalidParams(format!(
            "`{other}` is no driver command; the commands are find"
        ))),
        Err(error) => Err(error),
    };
    Task::ready(answer)
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
