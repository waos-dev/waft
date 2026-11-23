//! `cargo waft ext`: calls a service extension of a running debug app, as Flutter's tools call an
//! app's VM service, and lists the apps running.
//!
//! Running apps are found through `waft_service_protocol`'s discovery; a listing whose app no
//! longer answers was left by a process that ended without removing it, and is removed here.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use std::io::Write;

use anyhow::{Context, Result, bail};
use base64::Engine;
use serde_json::Value;
use waft_service_protocol::{Client, ListedApp, ListingFolder, discover};

/// Prints the apps running, one per line.
pub(crate) fn list() -> Result<()> {
    let apps = running();
    if apps.is_empty() {
        println!("No Waft app is running in a debug build on this machine.");
    }
    for (app, _) in apps {
        println!("{}  (process {})", app.app, app.pid);
    }
    Ok(())
}

/// Calls `name` with `params` (`name=value` each) on the app `app` names, or on the only one
/// running, and prints its answer: a dump's text as it is, anything else as JSON.
pub(crate) fn call(app: Option<&str>, name: &str, params: &[String]) -> Result<()> {
    let params = params
        .iter()
        .map(|param| {
            param
                .split_once('=')
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .with_context(|| format!("a parameter is `name=value`, not `{param}`"))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let (listed, mut client) = chosen(app)?;
    if !matches!(name, FIRST_FRAME | "ext.waft.listServiceExtensions") {
        wait_for_the_first_frame(&mut client);
    }
    let answer = client
        .call(name, params)
        .with_context(|| format!("calling {} (process {})", listed.app, listed.pid))?;
    match answer {
        Ok(result) => {
            match png_of(&result) {
                Some(png) => std::io::stdout()
                    .write_all(&png)
                    .context("writing the PNG to standard output")?,
                None => print!("{}", answer_text(&result)),
            }
            Ok(())
        }
        Err(error) => bail!("{} answered: {error}", listed.app),
    }
}

/// The extension that says whether the app has drawn its first frame, as Flutter's does.
const FIRST_FRAME: &str = "ext.waft.didSendFirstFrameEvent";

/// Waits until the app has drawn its first frame, as Flutter's tools wait before they call an
/// app: an app just started answers a dump before its tree is built. An app that does not say,
/// or has not drawn one in ten seconds, is called without waiting longer.
fn wait_for_the_first_frame(client: &mut Client) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match client.call(FIRST_FRAME, BTreeMap::new()) {
            Ok(Ok(answer)) if answer["enabled"] == "false" => {
                std::thread::sleep(Duration::from_millis(100));
            }
            _ => return,
        }
    }
}

/// The PNG a screenshot answers, `{"result": base64}`, decoded: written as it is, so
/// `> widget.png` keeps it.
fn png_of(result: &Value) -> Option<Vec<u8>> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    let encoded = result.get("result")?.as_str()?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    bytes.starts_with(SIGNATURE).then_some(bytes)
}

/// A dump's `{"data": text}` as its text; any other answer as JSON, on lines of its own.
fn answer_text(result: &Value) -> String {
    let only_data = result.as_object().is_some_and(|fields| fields.len() == 1);
    match result.get("data").and_then(Value::as_str) {
        Some(text) if only_data => text.to_owned(),
        _ => format!(
            "{}
",
            serde_json::to_string_pretty(result).unwrap_or_default()
        ),
    }
}

/// The app `app` names (by name or process id), or the only one running, connected.
fn chosen(app: Option<&str>) -> Result<(ListedApp, Client)> {
    let mut apps = running();
    if let Some(wanted) = app {
        apps.retain(|(listed, _)| listed.app == wanted || listed.pid.to_string() == wanted);
    }
    match apps.len() {
        1 => Ok(apps.remove(0)),
        0 => match app {
            Some(wanted) => {
                bail!("no running Waft app is named `{wanted}`; `cargo waft ext --list` lists them")
            }
            None => bail!("no Waft app is running in a debug build on this machine"),
        },
        _ => {
            let names: Vec<String> = apps
                .iter()
                .map(|(listed, _)| format!("{} ({})", listed.app, listed.pid))
                .collect();
            bail!(
                "several apps are running; choose one with --app: {}",
                names.join(", ")
            )
        }
    }
}

/// The apps listed that answer, each connected; the listings of those that do not are removed.
fn running() -> Vec<(ListedApp, Client)> {
    let folder = ListingFolder::new();
    discover(&[&folder])
        .into_iter()
        .filter_map(|listed| match Client::connect(&listed) {
            Ok(client) => Some((listed, client)),
            Err(_) => {
                // Left by an app that ended without removing it; only this machine's are files.
                if listed.device.is_none() {
                    let _ = std::fs::remove_file(listed.file_in(folder.dir()));
                }
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_screenshots_png_is_found_in_its_answer() {
        let png = b"\x89PNG\r\n\x1a\nrest".to_vec();
        let encoded = base64::engine::general_purpose::STANDARD.encode(&png);
        assert_eq!(png_of(&json!({ "result": encoded })), Some(png));
        assert_eq!(png_of(&json!({ "result": null })), None);
        assert_eq!(png_of(&json!({ "data": "[root]" })), None);
    }

    #[test]
    fn a_dump_prints_as_its_text_and_anything_else_as_json() {
        assert_eq!(answer_text(&json!({ "data": "[root]\n" })), "[root]\n");
        assert_eq!(
            answer_text(&json!({ "extensions": ["ext.waft.debugDumpApp"] })),
            "{\n  \"extensions\": [\n    \"ext.waft.debugDumpApp\"\n  ]\n}\n"
        );
    }
}
