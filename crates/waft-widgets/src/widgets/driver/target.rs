//! The widget a command names: by a CSS selector (`selector=`) or by the id `tree` printed
//! (`id=`), waited for as flutter_driver's `waitForElement` waits for its finder, and for a touch,
//! only where a touch would reach it (flutter_test's `hitTestable`).

use serde_json::Value;
use waft_embedder::{Offset, ServiceExtensionError, ServiceExtensionParams, ViewId};
use waft_foundation::{App, AsyncApp, DiagnosticsNode};

use super::controller::{self, WAIT_LIMIT};
use super::selector::WidgetSelector;
use crate::widgets::widget_inspector::{
    InspectorSerializationDelegate, element_named, from_inspector_id,
};
use crate::{AnyElement, WidgetsBinding};

/// A widget a command acts on, and where a touch on it lands.
pub(super) struct Target {
    pub(super) element: AnyElement,
    pub(super) at: Offset,
    pub(super) view: ViewId,
}

/// Whether a command touches its widget, and so needs one a touch would reach.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Reach {
    /// The widget a touch at its centre reaches: flutter_test's `hitTestable`.
    Touch,
    /// The widget, wherever it is.
    Any,
}

/// The one widget the command names, waited for until it is there, and with [`Reach::Touch`]
/// until a touch at its centre reaches it, for up to [`WAIT_LIMIT`]: flutter_driver's
/// `waitForElement`. Refused with why where it names none, several, or one a touch misses.
pub(super) async fn target(
    cx: &mut AsyncApp,
    params: &ServiceExtensionParams,
    reach: Reach,
) -> Result<Target, ServiceExtensionError> {
    let selector = Selection::of(params)?;
    controller::wait_until(cx, WAIT_LIMIT, |app| {
        !candidates(app, &selector, reach).is_empty()
    })
    .await;
    cx.update(|app| the_one(app, &selector, reach))
}

/// What a command names its widget by.
pub(super) enum Selection {
    Css(WidgetSelector, String),
    Id(String),
}

impl Selection {
    /// The command's `selector=` or `id=`, one of them.
    pub(super) fn of(params: &ServiceExtensionParams) -> Result<Selection, ServiceExtensionError> {
        match (params.get("selector"), params.get("id")) {
            (Some(css), None) => WidgetSelector::parse(css)
                .map(|selector| Selection::Css(selector, css.to_owned()))
                .map_err(ServiceExtensionError::InvalidParams),
            (None, Some(id)) => Ok(Selection::Id(id.to_owned())),
            _ => Err(ServiceExtensionError::InvalidParams(
                "a command names its widget by `selector` or by `id`, one of them".to_owned(),
            )),
        }
    }

    /// The selector or the id, as the command gave it.
    fn text(&self) -> &str {
        match self {
            Selection::Css(_, css) => css,
            Selection::Id(id) => id,
        }
    }

    /// The onstage widgets it names, in the tree's order.
    pub(super) fn named(&self, app: &mut App) -> Vec<AnyElement> {
        self.named_where(app, true)
    }

    /// The widgets it names, onstage or not: with `onstage`, those on screen alone.
    fn named_where(&self, app: &mut App, onstage: bool) -> Vec<AnyElement> {
        let Some(root) = WidgetsBinding::instance(app).root_element(app) else {
            return Vec::new();
        };
        match self {
            Selection::Css(selector, _) => selector.select(app, root, onstage),
            Selection::Id(id) => from_inspector_id(id)
                .and_then(|id| element_named(app, root, id))
                .into_iter()
                .collect(),
        }
    }
}

/// The widgets `selection` names that `reach` allows.
fn candidates(app: &mut App, selection: &Selection, reach: Reach) -> Vec<Target> {
    selection
        .named(app)
        .into_iter()
        .filter_map(|element| {
            let (at, view) = controller::center_of(app, element)?;
            let target = Target { element, at, view };
            (reach == Reach::Any || reaches(app, &target)).then_some(target)
        })
        .collect()
}

/// Whether a touch at the target's centre reaches its render object.
fn reaches(app: &mut App, target: &Target) -> bool {
    let Some(render) = target.element.find_render_object(app) else {
        return false;
    };
    controller::hit_at(app, target.at, target.view)
        .iter()
        .any(|hit| hit.id() == render.id())
}

/// The one widget `selection` names that `reach` allows, or why there is none.
fn the_one(
    app: &mut App,
    selection: &Selection,
    reach: Reach,
) -> Result<Target, ServiceExtensionError> {
    let naming = selection.text();
    let mut found = candidates(app, selection, reach);
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => {
            let named = selection.named(app);
            let missed = named.first().copied();
            Err(ServiceExtensionError::Failed(match missed {
                Some(element) if reach == Reach::Touch => missed_touch(app, naming, element),
                _ => named_nothing(app, selection),
            }))
        }
        count => {
            let lines: Vec<String> = found
                .iter()
                .map(|target| line_of(app, target.element))
                .collect();
            Err(ServiceExtensionError::Failed(format!(
                "`{naming}` names {count} widgets; name one by a narrower selector or its id:\n  {}",
                lines.join("\n  ")
            )))
        }
    }
}

/// Why `selection` names nothing on screen: nothing at all, or only widgets built off screen, a
/// page behind the one showing or a list's item outside its view, which a command does not see,
/// as flutter_driver's finders skip offstage widgets.
fn named_nothing(app: &mut App, selection: &Selection) -> String {
    let naming = selection.text();
    let seconds = WAIT_LIMIT.as_secs();
    match selection.named_where(app, false).len() {
        0 => format!("`{naming}` names no widget after {seconds} seconds"),
        offstage => format!(
            "`{naming}` names no widget on screen after {seconds} seconds, though it names \
             {offstage} built off screen, on a page behind this one or in a list outside its \
             view; `cargo waft ext scroll-until-visible '{naming}' --in <list> --dy <step>` \
             scrolls a list's item onto the screen"
        ),
    }
}

/// Why a touch on `element` would miss it: what it lands on instead.
fn missed_touch(app: &mut App, naming: &str, element: AnyElement) -> String {
    let line = line_of(app, element);
    let landed = controller::center_of(app, element)
        .and_then(|(at, view)| controller::hit_at(app, at, view).first().copied())
        .map_or("nothing".to_owned(), |hit| hit.to_string_short(app));
    format!(
        "`{naming}` names {line}, but a touch at its centre lands on {landed}: it is covered, \
         outside its view, or ignores touches"
    )
}

/// `element` as one line, as `cargo waft ext tree` prints it: its description, the line that
/// made it and its id.
pub(super) fn line_of(app: &App, element: AnyElement) -> String {
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
pub(super) fn node_of(app: &App, element: AnyElement) -> Value {
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
