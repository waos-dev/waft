//! Driving a running app as its user would, through its `ext.waft.driver` extension: flutter_driver's
//! commands, each naming its widget by a CSS selector or by the id `tree` printed.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use serde_json::Value;

use super::Connected;
use super::outline;

/// One app's driver, over one connection.
pub(super) struct Driver(Connected);

impl Driver {
    pub(super) fn to(app: Option<&str>) -> Result<Driver> {
        Ok(Driver(Connected::to(app)?))
    }

    /// The driver's answer to `command`, naming `widget` where given, with `params`.
    fn command(
        &mut self,
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
        self.0.call("ext.waft.driver", sent)
    }

    /// The widgets `widget` names now.
    fn found(&mut self, widget: &str) -> Result<Vec<Value>> {
        let answer = self.command("find", Some(widget), &[])?;
        Ok(answer["found"].as_array().cloned().unwrap_or_default())
    }

    /// The id of the one widget `widget` names: itself when it is one.
    fn id_of(&mut self, widget: &str) -> Result<String> {
        if naming(widget).0 == "id" {
            return Ok(widget.to_owned());
        }
        let found = self.found(widget)?;
        match found.as_slice() {
            [node] => Ok(node["valueId"].as_str().unwrap_or_default().to_owned()),
            [] => bail!("`{widget}` names no widget"),
            several => {
                let lines: Vec<String> = several.iter().map(line).collect();
                bail!(
                    "`{widget}` names {} widgets; name one by a narrower selector or its id:\n  {}",
                    several.len(),
                    lines.join("\n  ")
                )
            }
        }
    }
}

/// What a widget argument is: an id `tree` printed, or a CSS selector.
fn naming(widget: &str) -> (&'static str, String) {
    let key = if widget.starts_with("inspector-") {
        "id"
    } else {
        "selector"
    };
    (key, widget.to_owned())
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

/// Prints that the app answers, as flutter_driver's `checkHealth`.
pub(super) fn health(app: Option<&str>) -> Result<()> {
    let answer = Driver::to(app)?.command("get_health", None, &[])?;
    println!("{}", answer["status"].as_str().unwrap_or("?"));
    Ok(())
}

/// Prints each widget `widget` names, one a line; an error when it names none, as `grep` exits
/// when nothing matches.
pub(super) fn find(app: Option<&str>, widget: &str) -> Result<()> {
    let found = Driver::to(app)?.found(widget)?;
    if found.is_empty() {
        bail!("`{widget}` names no widget");
    }
    for node in &found {
        println!("{}", line(node));
    }
    Ok(())
}

/// Taps the one widget `widget` names.
pub(super) fn tap(app: Option<&str>, widget: &str, unsynchronized: bool) -> Result<()> {
    let params: Vec<_> = frame_sync(unsynchronized).into_iter().collect();
    let answer = Driver::to(app)?.command("tap", Some(widget), &params)?;
    println!("Tapped {}{}", line(&answer["tapped"]), unsettled(&answer));
    Ok(())
}

/// Holds a touch on the one widget `widget` names, past the long-press timeout.
pub(super) fn long_press(app: Option<&str>, widget: &str, unsynchronized: bool) -> Result<()> {
    let params: Vec<_> = frame_sync(unsynchronized).into_iter().collect();
    let answer = Driver::to(app)?.command("long_press", Some(widget), &params)?;
    println!("Pressed {}{}", line(&answer["pressed"]), unsettled(&answer));
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
    let answer = Driver::to(app)?.command("enter_text", widget, &params)?;
    println!("Entered {:?}{}", text, unsettled(&answer));
    Ok(())
}

/// Sends the focused field the keyboard's action `action`, as `done` or `search`.
pub(super) fn text_input_action(
    app: Option<&str>,
    action: &str,
    unsynchronized: bool,
) -> Result<()> {
    let mut params = vec![("action", action.to_owned())];
    params.extend(frame_sync(unsynchronized));
    let answer = Driver::to(app)?.command("send_text_input_action", None, &params)?;
    println!("Sent {action}{}", unsettled(&answer));
    Ok(())
}

/// Drags the one widget `widget` names by `by`, over `duration` milliseconds.
pub(super) fn scroll(
    app: Option<&str>,
    widget: &str,
    by: (f64, f64),
    duration: u64,
    unsynchronized: bool,
) -> Result<()> {
    let mut params = scroll_params(by, duration);
    params.extend(frame_sync(unsynchronized));
    let answer = Driver::to(app)?.command("scroll", Some(widget), &params)?;
    println!(
        "Scrolled {}{}",
        line(&answer["scrolled"]),
        unsettled(&answer)
    );
    Ok(())
}

fn scroll_params(by: (f64, f64), duration: u64) -> Vec<(&'static str, String)> {
    vec![
        ("dx", by.0.to_string()),
        ("dy", by.1.to_string()),
        ("duration", duration.to_string()),
    ]
}

/// Scrolls the scrollables around the one widget `widget` names until it shows, at `alignment`.
pub(super) fn scroll_into_view(app: Option<&str>, widget: &str, alignment: f64) -> Result<()> {
    let params = [("alignment", alignment.to_string())];
    let answer = Driver::to(app)?.command("scroll_into_view", Some(widget), &params)?;
    println!("Showed {}{}", line(&answer["shown"]), unsettled(&answer));
    Ok(())
}

/// Drags `scrollable` by `by` until `item` is on screen, then scrolls it into view at
/// `alignment`, for up to `timeout` milliseconds: flutter_driver's `scrollUntilVisible`, which
/// scrolls in steps a lazy list builds its items in.
pub(super) fn scroll_until_visible(
    app: Option<&str>,
    item: &str,
    scrollable: &str,
    by: (f64, f64),
    alignment: f64,
    timeout: u64,
) -> Result<()> {
    if by == (0.0, 0.0) {
        bail!("give the step to scroll by: --dx or --dy");
    }
    let mut driver = Driver::to(app)?;
    let deadline = Instant::now() + Duration::from_millis(timeout);
    while driver.found(item)?.is_empty() {
        if Instant::now() >= deadline {
            bail!("`{item}` did not show within {timeout} ms of scrolling `{scrollable}`");
        }
        driver.command("scroll", Some(scrollable), &scroll_params(by, 100))?;
    }
    let params = [("alignment", alignment.to_string())];
    let answer = driver.command("scroll_into_view", Some(item), &params)?;
    println!("Showed {}{}", line(&answer["shown"]), unsettled(&answer));
    Ok(())
}

/// Prints where the point `offset_type` of the one widget `widget` names is on screen.
pub(super) fn offset(app: Option<&str>, widget: &str, offset_type: &str) -> Result<()> {
    let params = [("offsetType", offset_type.to_owned())];
    let answer = Driver::to(app)?.command("get_offset", Some(widget), &params)?;
    println!(
        "({}, {}) in view {}",
        answer["dx"], answer["dy"], answer["view"]
    );
    Ok(())
}

/// Waits until `widget` names a widget, or with `absent` none, for up to `timeout` milliseconds.
pub(super) fn wait_for(app: Option<&str>, widget: &str, absent: bool, timeout: u64) -> Result<()> {
    let params = [
        ("absent", absent.to_string()),
        ("timeout", timeout.to_string()),
    ];
    let answer = Driver::to(app)?.command("wait_for", Some(widget), &params)?;
    let found = answer["found"].as_array().cloned().unwrap_or_default();
    if found.is_empty() {
        println!("`{widget}` names no widget.");
    }
    for node in &found {
        println!("{}", line(node));
    }
    Ok(())
}

/// Prints the text the one widget `widget` names shows.
pub(super) fn get_text(app: Option<&str>, widget: &str) -> Result<()> {
    let answer = Driver::to(app)?.command("get_text", Some(widget), &[])?;
    println!("{}", answer["text"].as_str().unwrap_or_default());
    Ok(())
}

/// Prints the layout of the one widget `widget` names and of its children: each one's size,
/// constraints and place, as DevTools' layout explorer shows them.
pub(super) fn layout(app: Option<&str>, widget: &str, json: bool) -> Result<()> {
    let mut driver = Driver::to(app)?;
    let id = driver.id_of(widget)?;
    let params = BTreeMap::from([
        ("id".to_owned(), id),
        ("groupName".to_owned(), "cli".to_owned()),
        ("subtreeDepth".to_owned(), "1".to_owned()),
    ]);
    let answer = driver
        .0
        .call("ext.waft.inspector.getLayoutExplorerNode", params)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&answer["result"])?);
    } else {
        print!("{}", outline::layout_outline(&answer["result"]));
    }
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
