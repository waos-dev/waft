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

    #[test]
    fn a_long_property_is_cut_to_fit() {
        let long = "x".repeat(60);
        let node = json!({ "description": "Text", "properties": [{ "name": "data", "description": long }] });
        let properties = key_properties(&node);
        assert_eq!(properties.chars().count(), "data: ".len() + PROPERTY_WIDTH);
        assert!(properties.ends_with('…'));
    }
}
