//! Driving a running app as its user would, through its `ext.waft.driver` extension: flutter_driver's
//! commands, each naming its widget by a CSS selector or by the id `tree` printed.

use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::Value;

use super::Connected;
use super::outline;

/// What a widget argument is: an id `tree` printed, or a CSS selector.
fn naming(widget: &str) -> (&'static str, String) {
    let key = if widget.starts_with("inspector-") {
        "id"
    } else {
        "selector"
    };
    (key, widget.to_owned())
}

/// The driver's answer to `command`, naming `widget` where given, with `params`.
fn drive(
    app: Option<&str>,
    command: &str,
    widget: Option<&str>,
    params: &[(&str, String)],
) -> Result<Value> {
    let mut sent = BTreeMap::from([("command".to_owned(), command.to_owned())]);
    if let Some(widget) = widget {
        let (key, value) = naming(widget);
        sent.insert(key.to_owned(), value);
    }
    for (name, value) in params {
        sent.insert((*name).to_owned(), value.clone());
    }
    Connected::to(app)?.call("ext.waft.driver", sent)
}

/// The widget a node names, as `tree` prints its line.
fn line(node: &Value) -> String {
    outline::outline(node).trim_end().to_owned()
}

/// What to say when the app was still drawing five seconds after the command acted: what it
/// shows may still change.
fn unsettled(answer: &Value) -> &'static str {
    if answer["settled"] == false {
        "\n(the app was still drawing 5 seconds after; what it shows may still change)"
    } else {
        ""
    }
}

/// The parameter that turns frame sync off: flutter_driver's `runUnsynchronized`.
fn frame_sync(unsynchronized: bool) -> Option<(&'static str, String)> {
    unsynchronized.then(|| ("frameSync", "false".to_owned()))
}

/// Prints each widget `widget` names, one a line.
pub(super) fn find(app: Option<&str>, widget: &str) -> Result<()> {
    let answer = drive(app, "find", Some(widget), &[])?;
    print_found(&answer, widget);
    Ok(())
}

fn print_found(answer: &Value, widget: &str) {
    let found = answer["found"].as_array().cloned().unwrap_or_default();
    if found.is_empty() {
        println!("`{widget}` names no widget.");
    }
    for node in &found {
        println!("{}", line(node));
    }
}

/// Taps the one widget `widget` names.
pub(super) fn tap(app: Option<&str>, widget: &str, unsynchronized: bool) -> Result<()> {
    let params: Vec<_> = frame_sync(unsynchronized).into_iter().collect();
    let answer = drive(app, "tap", Some(widget), &params)?;
    println!("Tapped {}{}", line(&answer["tapped"]), unsettled(&answer));
    Ok(())
}

/// Types `text` into the field `widget` names, or into the focused field.
pub(super) fn enter_text(
    app: Option<&str>,
    widget: Option<&str>,
    text: &str,
    unsynchronized: bool,
) -> Result<()> {
    let mut params = vec![("text", text.to_owned())];
    params.extend(frame_sync(unsynchronized));
    let answer = drive(app, "enter_text", widget, &params)?;
    println!("Entered {:?}{}", text, unsettled(&answer));
    Ok(())
}

/// Drags the one widget `widget` names by `dx`, `dy` over `duration` milliseconds.
pub(super) fn scroll(
    app: Option<&str>,
    widget: &str,
    by: (f64, f64),
    duration: u64,
    unsynchronized: bool,
) -> Result<()> {
    let mut params = vec![
        ("dx", by.0.to_string()),
        ("dy", by.1.to_string()),
        ("duration", duration.to_string()),
    ];
    params.extend(frame_sync(unsynchronized));
    let answer = drive(app, "scroll", Some(widget), &params)?;
    println!(
        "Scrolled {}{}",
        line(&answer["scrolled"]),
        unsettled(&answer)
    );
    Ok(())
}

/// Waits until `widget` names a widget, or with `absent` none, for up to `timeout` milliseconds.
pub(super) fn wait_for(app: Option<&str>, widget: &str, absent: bool, timeout: u64) -> Result<()> {
    let params = [
        ("absent", absent.to_string()),
        ("timeout", timeout.to_string()),
    ];
    let answer = drive(app, "wait_for", Some(widget), &params)?;
    print_found(&answer, widget);
    Ok(())
}

/// Prints the text the one widget `widget` names shows.
pub(super) fn get_text(app: Option<&str>, widget: &str) -> Result<()> {
    let answer = drive(app, "get_text", Some(widget), &[])?;
    println!("{}", answer["text"].as_str().unwrap_or_default());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_widget_is_named_by_its_id_or_a_selector() {
        assert_eq!(naming("inspector-4294967318").0, "id");
        assert_eq!(
            naming(r#"ListTile:has([text="Wi-Fi"]) Switch"#).0,
            "selector"
        );
    }
}
