//! A tree of the inspector's JSON as an outline, for a person or an agent: one widget a line,
//! indented under its parent, with the text it shows, its properties where the tree carries them,
//! the file and line that made it, and its id.
//!
//! ```text
//! Scaffold  crates/notes/src/home.rs:18  inspector-4294967307
//!  Text "Search notes"  crates/notes/src/home.rs:22  inspector-4294967309
//!  Padding (padding: EdgeInsets(16.0, 20.0, 8.0, 8.0))  crates/notes/src/home.rs:24  inspector-4294967310
//! ```
//!
//! Which widgets it holds is the framework's to say: the summary tree keeps the ones the app's
//! own code made, as Flutter's inspector does.

use serde_json::Value;

/// An element's properties that say how it is placed in the tree rather than what its widget
/// shows, left out of a line.
const ELEMENT_PROPERTIES: &[&str] = &["dependencies", "renderObject", "state"];

/// The longest a property's text is shown, and all of a line's properties together.
const PROPERTY_WIDTH: usize = 48;
const PROPERTIES_WIDTH: usize = 120;

/// `node` and its descendants as an outline.
pub(crate) fn outline(node: &Value) -> String {
    let mut text = String::new();
    write(node, 0, &mut text);
    text
}

fn write(node: &Value, depth: usize, text: &mut String) {
    text.push_str(&line(node, depth));
    for child in node["children"].as_array().into_iter().flatten() {
        write(child, depth + 1, text);
    }
}

/// One widget's line.
fn line(node: &Value, depth: usize) -> String {
    let mut line = " ".repeat(depth);
    line.push_str(node["description"].as_str().unwrap_or("?"));
    if let Some(preview) = node["textPreview"].as_str() {
        line.push_str(&format!(" {:?}", cut(preview, PROPERTY_WIDTH)));
    }
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
            let shows_name = property["showName"] != false;
            let description = property["description"].as_str()?;
            // A nameless property with no value says nothing.
            if !shows_name && description == "null" {
                return None;
            }
            let description = cut(description, PROPERTY_WIDTH);
            Some(match property["name"].as_str() {
                Some(name) if shows_name => format!("{name}: {description}"),
                _ => description,
            })
        })
        .collect();
    cut(&properties.join(", "), PROPERTIES_WIDTH)
}

/// A layout explorer node and its children as lines: each widget with its size, constraints and
/// place in its parent, the render object that lays it out, the line that made it and its id.
///
/// ```text
/// Padding  40.0×20.0  BoxConstraints(0.0<=w<=800.0, 0.0<=h<=600.0)  at (380.0, 290.0)  RenderPadding#00a3f  crates/notes/src/home.rs:24  inspector-4294967310
/// ```
pub(crate) fn layout_outline(node: &Value) -> String {
    let mut text = String::new();
    write_layout(node, 0, &mut text);
    text
}

fn write_layout(node: &Value, depth: usize, text: &mut String) {
    let mut line = " ".repeat(depth);
    line.push_str(node["description"].as_str().unwrap_or("?"));
    if let Some(size) = node["size"].as_object() {
        let side = |name: &str| size.get(name).and_then(Value::as_str).unwrap_or("?");
        line.push_str(&format!("  {}×{}", side("width"), side("height")));
    }
    if let Some(constraints) = node["constraints"]["description"].as_str() {
        line.push_str(&format!("  {constraints}"));
    }
    if let Some(flex) = node["flexFactor"].as_i64() {
        let fit = node["flexFit"].as_str().unwrap_or("?");
        line.push_str(&format!("  flex {flex} {fit}"));
    } else if let Some(offset) = node["parentData"].as_object() {
        let at = |name: &str| offset.get(name).and_then(Value::as_str).unwrap_or("?");
        line.push_str(&format!("  at ({}, {})", at("offsetX"), at("offsetY")));
    }
    if let Some(render) = node["renderObject"]["description"].as_str() {
        line.push_str(&format!("  {render}"));
    }
    if let Some(location) = node["creationLocation"].as_object() {
        let file = location.get("file").and_then(Value::as_str).unwrap_or("?");
        let at = location.get("line").and_then(Value::as_u64).unwrap_or(0);
        line.push_str(&format!("  {file}:{at}"));
    }
    if let Some(id) = node["valueId"].as_str() {
        line.push_str(&format!("  {id}"));
    }
    text.push_str(&line);
    text.push('\n');
    for child in node["children"].as_array().into_iter().flatten() {
        write_layout(child, depth + 1, text);
    }
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

    /// A summary tree, as the framework gives it: the app's own widgets, a paragraph's text
    /// previewed.
    fn summary() -> Value {
        json!({
            "description": "[root]",
            "valueId": "inspector-1",
            "children": [{
                "description": "Home",
                "creationLocation": { "file": "crates/notes/src/app.rs", "line": 12 },
                "valueId": "inspector-2",
                "children": [{
                    "description": "Text",
                    "textPreview": "Search notes",
                    "creationLocation": { "file": "crates/notes/src/home.rs", "line": 22 },
                    "valueId": "inspector-3"
                }]
            }]
        })
    }

    /// Every widget the tree holds, under its parent, with the text it shows, its line and id.
    #[test]
    fn each_widget_is_a_line_under_its_parent() {
        assert_eq!(
            outline(&summary()),
            "[root]  inspector-1\n \
             Home  crates/notes/src/app.rs:12  inspector-2\n  \
             Text \"Search notes\"  crates/notes/src/home.rs:22  inspector-3\n"
        );
    }

    /// A widget's details carry its properties, shown but for the element's own.
    #[test]
    fn a_widgets_details_show_its_properties() {
        let details = json!({
            "description": "Padding",
            "valueId": "inspector-4",
            "properties": [
                { "name": "padding", "description": "EdgeInsets.all(16.0)" },
                { "name": "renderObject", "description": "RenderPadding#00a3f" }
            ]
        });
        assert_eq!(
            outline(&details),
            "Padding (padding: EdgeInsets.all(16.0))  inspector-4\n"
        );
    }

    /// A layout node is its size, constraints, place and render object, its children under it.
    #[test]
    fn a_layout_is_a_line_a_widget() {
        let node = json!({
            "description": "Padding",
            "size": { "width": "40.0", "height": "20.0" },
            "constraints": { "description": "BoxConstraints(0.0<=w<=800.0, 0.0<=h<=600.0)" },
            "parentData": { "offsetX": "380.0", "offsetY": "290.0" },
            "renderObject": { "description": "RenderPadding#00a3f" },
            "valueId": "inspector-4",
            "children": [{
                "description": "Expanded",
                "size": { "width": "40.0", "height": "10.0" },
                "flexFactor": 1,
                "flexFit": "tight",
                "valueId": "inspector-5"
            }]
        });
        assert_eq!(
            layout_outline(&node),
            "Padding  40.0×20.0  BoxConstraints(0.0<=w<=800.0, 0.0<=h<=600.0)  at (380.0, 290.0)  \
             RenderPadding#00a3f  inspector-4\n Expanded  40.0×10.0  flex 1 tight  inspector-5\n"
        );
    }

    /// A property shown without its name says nothing when it has no value.
    #[test]
    fn a_nameless_property_without_a_value_is_left_out() {
        let node = json!({ "description": "ListView", "properties": [
            { "name": "scrollDirection", "description": "vertical" },
            { "name": "controller", "description": "null", "showName": false }
        ]});
        assert_eq!(key_properties(&node), "scrollDirection: vertical");
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
