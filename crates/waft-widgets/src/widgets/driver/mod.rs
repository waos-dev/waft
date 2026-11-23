//! `ext.waft.driver`: flutter_driver's commands, which a tool sends a running app to act as its
//! user would: find widgets, tap, type, scroll, wait and read text. Registered in every debug
//! build, where flutter_driver's need the app to call `enableFlutterDriverExtension` first; it
//! changes nothing until a command arrives.
//!
//! A command names its widget by a CSS selector (`selector=…`, see [`selector`]) or by the id
//! `cargo waft ext tree` printed (`id=inspector-…`), for this run of the app.
//!
//! A command that acts first waits until the app has settled, flutter_driver's frame sync, and
//! fails when it does not within five seconds, as on a screen with a spinner; `frameSync=false`
//! acts without waiting, flutter_driver's `runUnsynchronized`. Having acted, it waits again, so
//! its answer comes once what it did shows, and says whether the app settled then.

mod controller;
mod selector;

use std::rc::Rc;
use std::time::Duration;

use serde_json::{Value, json};
use waft_embedder::{
    Offset, ServiceExtensionError, ServiceExtensionParams, ServiceExtensionResult, TextAffinity,
    TextEditingValue, TextSelection, ViewId,
};
use waft_foundation::{App, AsyncApp, DiagnosticsNode, ServiceExtensions, Task, delayed};
use waft_services::TextInput;

use self::selector::WidgetSelector;
use crate::widgets::text::shown_text;
use crate::widgets::widget_inspector::{
    InspectorSerializationDelegate, element_named, from_inspector_id, number,
};
use crate::{AnyElement, WidgetsBinding};

/// Registers `ext.waft.driver`, beside the inspector's extensions.
pub(crate) fn init_service_extensions(app: &mut App) {
    ServiceExtensions::register(app, "ext.waft.driver", Rc::new(driver_command));
}

/// The commands, as `command=` names them.
const COMMANDS: &str = "find, tap, enter_text, scroll, wait_for, get_text";

/// How long `wait_for` waits when not told, as flutter_driver's default timeout.
const WAIT_LIMIT: Duration = Duration::from_secs(5);

/// Runs the command `command=` names.
fn driver_command(app: &mut App, params: ServiceExtensionParams) -> Task<ServiceExtensionResult> {
    match params.require("command") {
        Ok("find") => Task::ready(find(app, &params)),
        Ok("tap") => app.spawn(async move |cx| tap(cx, &params).await),
        Ok("enter_text") => app.spawn(async move |cx| enter_text(cx, &params).await),
        Ok("scroll") => app.spawn(async move |cx| scroll(cx, &params).await),
        Ok("wait_for") => app.spawn(async move |cx| wait_for(cx, &params).await),
        Ok("get_text") => Task::ready(get_text(app, &params)),
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
/// false where the app was still drawing five seconds after the tap.
async fn tap(cx: &mut AsyncApp, params: &ServiceExtensionParams) -> ServiceExtensionResult {
    synchronize(cx, params).await?;
    let tapped = cx.update(|app| {
        let (element, at, view) = aimed_at(app, params)?;
        controller::tap_at(app, at, view);
        Ok::<_, ServiceExtensionError>(node_of(app, element))
    })?;
    let settled = controller::settle(cx).await;
    Ok(json!({ "tapped": tapped, "settled": settled }))
}

/// flutter_driver's frame sync before a command acts: waits until the app has settled, and
/// refuses the command where it has not within [`controller::SETTLE_LIMIT`]; with
/// `frameSync=false`, flutter_driver's `runUnsynchronized`, acts at once.
async fn synchronize(
    cx: &mut AsyncApp,
    params: &ServiceExtensionParams,
) -> Result<(), ServiceExtensionError> {
    if params.get("frameSync") == Some("false") || controller::settle(cx).await {
        return Ok(());
    }
    Err(ServiceExtensionError::Failed(format!(
        "the app did not settle within {} seconds: something keeps animating, as a spinner does. \
         To act without waiting, turn frame sync off: `--unsynchronized` with cargo waft ext, \
         `frameSync=false` in a call",
        controller::SETTLE_LIMIT.as_secs()
    )))
}

/// `enter_text`: `text=` as the focused field's whole text, the caret after it, as the platform's
/// text input sends a field its value (flutter_driver's `enter_text`); a widget the command names
/// is tapped first, to focus it. As `{"entered": text, "settled": bool}`.
async fn enter_text(cx: &mut AsyncApp, params: &ServiceExtensionParams) -> ServiceExtensionResult {
    let text = params.require("text")?.to_owned();
    synchronize(cx, params).await?;
    if params.get("selector").is_some() || params.get("id").is_some() {
        cx.update(|app| {
            let (_, at, view) = aimed_at(app, params)?;
            controller::tap_at(app, at, view);
            Ok::<_, ServiceExtensionError>(())
        })?;
        synchronize(cx, params).await?;
    }
    cx.update(|app| {
        let input = TextInput::instance(app);
        if input.current_connection(app).is_none() {
            return Err(ServiceExtensionError::Failed(
                "no text field has the focus: name the field to type into".to_owned(),
            ));
        }
        let caret = text.encode_utf16().count() as i32;
        let value = TextEditingValue::new()
            .text(text.clone())
            .selection(TextSelection::collapsed(caret, TextAffinity::Downstream));
        input.update_editing_value(app, value);
        Ok(())
    })?;
    let settled = controller::settle(cx).await;
    Ok(json!({ "entered": text, "settled": settled }))
}

/// `scroll`: a touch down at the centre of the one widget the command names, moved by `dx=`,
/// `dy=` logical pixels over `duration=` milliseconds (300 when not given) and lifted, as
/// flutter_driver's `scroll`. As `{"scrolled": node, "settled": bool}`.
async fn scroll(cx: &mut AsyncApp, params: &ServiceExtensionParams) -> ServiceExtensionResult {
    let by = Offset::new(
        number(params, "dx")?.unwrap_or(0.0),
        number(params, "dy")?.unwrap_or(0.0),
    );
    let duration = Duration::from_millis(number(params, "duration")?.unwrap_or(300.0) as u64);
    synchronize(cx, params).await?;
    let (element, at, view) = cx.update(|app| aimed_at(app, params))?;
    controller::drag(cx, at, view, by, duration).await;
    let settled = controller::settle(cx).await;
    let scrolled = cx.update(|app| node_of(app, element));
    Ok(json!({ "scrolled": scrolled, "settled": settled }))
}

/// `wait_for`: waits until the command names a widget, or with `absent=true` none, for up to
/// `timeout=` milliseconds (5000 when not given), as flutter_driver's `waitFor` and
/// `waitForAbsent`. As `{"found": [node, …]}`.
async fn wait_for(cx: &mut AsyncApp, params: &ServiceExtensionParams) -> ServiceExtensionResult {
    let absent = params.get("absent") == Some("true");
    let limit = number(params, "timeout")?.map_or(WAIT_LIMIT, |milliseconds| {
        Duration::from_millis(milliseconds as u64)
    });
    let frame = Duration::from_millis(16);
    let mut waited = Duration::ZERO;
    loop {
        let found = cx.update(|app| named(app, params))?;
        if found.is_empty() == absent {
            let found: Vec<Value> = cx.update(|app| {
                found
                    .into_iter()
                    .map(|element| node_of(app, element))
                    .collect()
            });
            return Ok(json!({ "found": found }));
        }
        if waited >= limit {
            let what = if absent {
                "still names a widget"
            } else {
                "names no widget"
            };
            return Err(ServiceExtensionError::Failed(format!(
                "`{}` {what} after {} ms",
                naming(params),
                limit.as_millis()
            )));
        }
        cx.update(|app| delayed(app, frame)).await;
        waited += frame;
    }
}

/// `get_text`: the text the one widget the command names shows, or the first text inside it, as
/// flutter_driver's `get_text` reads a `Text` or a field. As `{"text": text}`.
fn get_text(app: &mut App, params: &ServiceExtensionParams) -> ServiceExtensionResult {
    let element = the_one(app, params)?;
    let text = std::iter::once(element)
        .chain(element.descendants(app, true))
        .find_map(|element| shown_text(app, element))
        .ok_or_else(|| {
            ServiceExtensionError::Failed(format!("{} shows no text", line_of(app, element)))
        })?;
    Ok(json!({ "text": text }))
}

/// The one widget the command names, and where a touch on it lands: the centre of its box, in
/// the view it renders into.
fn aimed_at(
    app: &mut App,
    params: &ServiceExtensionParams,
) -> Result<(AnyElement, Offset, ViewId), ServiceExtensionError> {
    let element = the_one(app, params)?;
    let (at, view) = controller::center_of(app, element).ok_or_else(|| {
        ServiceExtensionError::Failed(format!("{} renders no box to touch", line_of(app, element)))
    })?;
    Ok((element, at, view))
}

/// How the command names its widget: its selector, or its id.
fn naming(params: &ServiceExtensionParams) -> &str {
    params
        .get("selector")
        .or(params.get("id"))
        .unwrap_or_default()
}

/// The one widget the command names; where it names none or several, why, with each widget's
/// line to choose one by.
fn the_one(
    app: &mut App,
    params: &ServiceExtensionParams,
) -> Result<AnyElement, ServiceExtensionError> {
    let mut named = named(app, params)?;
    let naming = naming(params);
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
