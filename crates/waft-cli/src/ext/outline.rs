//! The widget tree as an outline, for a person or an agent: one widget a line, with its key
//! properties, the file and line that made it, and its id.
//!
//! ```text
//! Scaffold  crates/notes/src/home.rs:18  inspector-4294967307
//!  Padding (padding: EdgeInsets(16.0, 20.0, 8.0, 8.0))  crates/notes/src/home.rs:24  inspector-4294967310
//! ```
//!
//! It is read from the inspector's details tree. By default it keeps the widgets the app's own
//! code made (`createdByLocalProject`), each indented under the nearest one it keeps.

use serde_json::Value;

/// An element's properties that say how it is placed in the tree rather than what its widget
/// shows, left out of a line.
const ELEMENT_PROPERTIES: &[&str] = &["dependencies", "renderObject", "state"];

/// The longest a property's text is shown, and all of a line's properties together.
const PROPERTY_WIDTH: usize = 48;
const PROPERTIES_WIDTH: usize = 120;

/// `node` and its descendants as an outline; with `all`, every widget, the framework's among them.
pub(crate) fn outline(node: &Value, all: bool) -> String {
    let mut text = String::new();
    write(node, all, 0, &mut text);
    text
}

fn write(node: &Value, all: bool, depth: usize, text: &mut String) {
    let shown = all || node["createdByLocalProject"] == true;
    if shown {
        text.push_str(&line(node, depth));
    }
    let child_depth = if shown { depth + 1 } else { depth };
    for child in node["children"].as_array().into_iter().flatten() {
        write(child, all, child_depth, text);
    }
}

/// One widget's line.
fn line(node: &Value, depth: usize) -> String {
    let mut line = " ".repeat(depth);
    line.push_str(node["description"].as_str().unwrap_or("?"));
    let properties = key_properties(node);
    if !properties.is_empty() {
        line.push_str(&format!(" ({properties})"));
    }
    if let Some(location) = node["creationLocation"].as_object() {
        let file = location.get("file").and_then(Value::as_str).unwrap_or("?");
        let at = location.get("line").and_then(Value::as_u64).unwrap_or(0);
        line.push_str(&format!("  {file}:{at}"));
    }
    if let Some(id) = node["valueId"].as_str() {
        line.push_str(&format!("  {id}"));
    }
    line.push('\n');
    line
}

/// The widget's properties the inspector shows, each `name: value`, cut to fit a line.
fn key_properties(node: &Value) -> String {
    let properties: Vec<String> = node["properties"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|property| {
            let name = property["name"].as_str().unwrap_or_default();
            !ELEMENT_PROPERTIES.contains(&name)
        })
        .filter_map(|property| {
            let description = cut(property["description"].as_str()?, PROPERTY_WIDTH);
            let shows_name = property["showName"] != false;
            Some(match property["name"].as_str() {
                Some(name) if shows_name => format!("{name}: {description}"),
                _ => description,
            })
        })
        .collect();
    cut(&properties.join(", "), PROPERTIES_WIDTH)
}

/// `text`, ending in `…` where it is longer than `width` characters.
fn cut(text: &str, width: usize) -> String {
    let text = text.replace('\n', " ");
    if text.chars().count() <= width {
        return text;
    }
    let kept: String = text.chars().take(width - 1).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn tree() -> Value {
        json!({
            "description": "[root]",
            "valueId": "inspector-1",
            "children": [{
                "description": "Home",
                "createdByLocalProject": true,
                "creationLocation": { "file": "crates/notes/src/app.rs", "line": 12 },
                "valueId": "inspector-2",
                "properties": [
                    { "name": "state", "description": "HomeState#1a2b3" }
                ],
                "children": [{
                    "description": "Semantics",
                    "creationLocation": { "file": "/home/me/.cargo/registry/src/waft-widgets/src/basic.rs", "line": 40 },
                    "valueId": "inspector-3",
                    "children": [{
                        "description": "Padding",
                        "createdByLocalProject": true,
                        "creationLocation": { "file": "crates/notes/src/home.rs", "line": 24 },
                        "valueId": "inspector-4",
                        "properties": [
                            { "name": "padding", "description": "EdgeInsets.all(16.0)" },
                            { "name": "renderObject", "description": "RenderPadding#00a3f" }
                        ]
                    }]
                }]
            }]
        })
    }

    /// The app's own widgets, each under the nearest one kept, with their properties, lines and
    /// ids; the framework's in between and the element's own properties left out.
    #[test]
    fn the_outline_keeps_the_apps_own_widgets() {
        assert_eq!(
            outline(&tree(), false),
            "Home  crates/notes/src/app.rs:12  inspector-2\n \
             Padding (padding: EdgeInsets.all(16.0))  crates/notes/src/home.rs:24  inspector-4\n"
        );
    }

    #[test]
    fn with_all_every_widget_is_kept() {
        let text = outline(&tree(), true);
        assert!(text.starts_with("[root]  inspector-1\n Home"), "{text}");
        assert!(
            text.contains("  Semantics  /home/me/.cargo/registry/"),
            "{text}"
        );
        assert!(
            text.contains("   Padding (padding: EdgeInsets.all(16.0))"),
            "{text}"
        );
    }

    #[test]
    fn a_long_property_is_cut_to_fit() {
        let long = "x".repeat(60);
        let node = json!({ "description": "Text", "properties": [{ "name": "data", "description": long }] });
        let properties = key_properties(&node);
        assert_eq!(properties.chars().count(), "data: ".len() + PROPERTY_WIDTH);
        assert!(properties.ends_with('…'));
    }
}
