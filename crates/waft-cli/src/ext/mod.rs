//! `cargo waft ext`: a running debug app's service extensions, as Flutter's tools reach an app's
//! VM service: the apps running, their trees, a widget's picture, driving it as its user would,
//! and any extension by name.
//!
//! Running apps are found through `waft_service_protocol`'s discovery: this machine's listing
//! folder, and the programs WaOS runs. A listing in the folder whose app no longer answers was
//! left by a process that ended without removing it, and is removed here.

mod drive;
mod outline;
mod waos;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use base64::Engine;
use clap::{Subcommand, ValueEnum};
use serde_json::Value;
use waft_service_protocol::{Client, ListedApp, ListingFolder, METHOD_NOT_FOUND, discover};

use self::waos::WaosPrograms;

/// What `cargo waft ext` does.
#[derive(Subcommand)]
pub(crate) enum ExtCommand {
    /// The Waft apps running in a debug build on this machine, WaOS's programs among them.
    List,
    /// A tree as text, as Flutter's `debugDump…` prints it.
    Dump {
        #[arg(value_enum)]
        tree: DumpedTree,
    },
    /// The widget tree as an outline, one widget a line: the widgets your project made, each with
    /// the text it shows, the file and line that made it, and the id `screenshot` takes; or, from
    /// a widget's id, its subtree with every widget's properties.
    Tree {
        /// The widget whose subtree is shown, by its id; the whole tree without one.
        id: Option<String>,
        /// How many levels below the widget the subtree goes.
        #[arg(long, requires = "id", default_value_t = 2)]
        depth: u32,
        /// Every widget, the framework's among them.
        #[arg(long, conflicts_with = "id")]
        all: bool,
        /// The inspector's JSON, as a tool reads it.
        #[arg(long)]
        json: bool,
    },
    /// A PNG of one widget alone, by its id from `tree`, or of a whole view, written to a file:
    /// `waft_01.png`, then `waft_02.png`, … in this folder, or the one `--out` names.
    Screenshot {
        /// The widget's id, as `inspector-4294967303`; a whole view without one.
        #[arg(conflicts_with = "view")]
        id: Option<String>,
        /// The view to picture whole, by its id; the app's first view without one.
        #[arg(long)]
        view: Option<u64>,
        /// Pixels to a point: 2 for a widget, and the view's own for the whole view, by default.
        #[arg(long)]
        scale: Option<f64>,
        /// The file to write.
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// The widgets WIDGET names, one a line, as `tree` prints them.
    Find {
        /// A CSS selector, as `ListTile:has([text="Wi-Fi"]) Switch`, or an id `tree` printed.
        widget: String,
    },
    /// Taps the one widget WIDGET names, at its centre, once the app has settled.
    Tap {
        /// A CSS selector, as `ListTile:has([text="Wi-Fi"]) Switch`, or an id `tree` printed.
        widget: String,
        /// Act at once, without waiting for the app to settle first: for a screen that never
        /// does, as one with a spinner (flutter_driver's `runUnsynchronized`).
        #[arg(long)]
        unsynchronized: bool,
    },
    /// Holds a touch on the one widget WIDGET names past the long-press timeout.
    LongPress {
        /// A CSS selector, as `ListTile:has([text="Wi-Fi"])`, or an id `tree` printed.
        widget: String,
        /// Act at once, without waiting for the app to settle first: for a screen that never
        /// does, as one with a spinner (flutter_driver's `runUnsynchronized`).
        #[arg(long)]
        unsynchronized: bool,
    },
    /// Types TEXT into the field WIDGET names, replacing its text; into the focused field, given
    /// TEXT alone.
    EnterText {
        /// The field, by a CSS selector or an id `tree` printed; or, alone, the text.
        #[arg(value_name = "WIDGET")]
        first: String,
        /// The text to type.
        text: Option<String>,
        /// Act at once, without waiting for the app to settle first: for a screen that never
        /// does, as one with a spinner (flutter_driver's `runUnsynchronized`).
        #[arg(long)]
        unsynchronized: bool,
    },
    /// Sends the focused field the keyboard's action key: `done`, `search`, `next`, `go`, `send`,
    /// `newline`, …, as pressing it submits a search or moves to the next field.
    TextInputAction {
        /// The action, by Flutter's `TextInputAction` name.
        action: String,
        /// Act at once, without waiting for the app to settle first: for a screen that never
        /// does, as one with a spinner (flutter_driver's `runUnsynchronized`).
        #[arg(long)]
        unsynchronized: bool,
    },
    /// Drags the one widget WIDGET names by DX, DY logical pixels, as a finger scrolls it.
    Scroll {
        /// A CSS selector, as `ListView`, or an id `tree` printed.
        widget: String,
        /// Pixels across: negative drags left, showing what is to the right.
        #[arg(long, default_value_t = 0.0, allow_negative_numbers = true)]
        dx: f64,
        /// Pixels down: negative drags up, showing what is below.
        #[arg(long, default_value_t = 0.0, allow_negative_numbers = true)]
        dy: f64,
        /// How long the drag takes, in milliseconds.
        #[arg(long, default_value_t = 300)]
        duration: u64,
        /// Act at once, without waiting for the app to settle first: for a screen that never
        /// does, as one with a spinner (flutter_driver's `runUnsynchronized`).
        #[arg(long)]
        unsynchronized: bool,
    },
    /// Scrolls the lists around the one widget WIDGET names until it shows: for a widget already
    /// partly on screen (flutter_driver's `scrollIntoView`).
    ScrollIntoView {
        /// A CSS selector, as `[key="item-12"]`, or an id `tree` printed.
        widget: String,
        /// Where along the list it shows: 0 at the start, 1 at the end.
        #[arg(long, default_value_t = 0.0)]
        alignment: f64,
    },
    /// Drags the list SCROLLABLE by DX, DY at a time until ITEM is on screen, then scrolls it
    /// into view: for an item a lazy list has not built yet (flutter_driver's
    /// `scrollUntilVisible`).
    ScrollUntilVisible {
        /// The item, by a CSS selector, as `[text="Settings"]`.
        item: String,
        /// The list to drag, by a CSS selector, as `ListView`, or an id `tree` printed.
        #[arg(long = "in", value_name = "SCROLLABLE")]
        scrollable: String,
        /// Pixels across a step: negative drags left, showing what is to the right.
        #[arg(long, default_value_t = 0.0, allow_negative_numbers = true)]
        dx: f64,
        /// Pixels down a step: negative drags up, showing what is below.
        #[arg(long, default_value_t = 0.0, allow_negative_numbers = true)]
        dy: f64,
        /// Where along the list the item shows: 0 at the start, 1 at the end.
        #[arg(long, default_value_t = 0.0)]
        alignment: f64,
        /// How long to keep scrolling, in milliseconds.
        #[arg(long, default_value_t = 10000)]
        timeout: u64,
    },
    /// Where the one widget WIDGET names is on screen: a point of it, in its view's logical
    /// pixels (flutter_driver's `get_offset`).
    Offset {
        /// A CSS selector, as `[text="Save"]`, or an id `tree` printed.
        widget: String,
        /// Which point of it.
        #[arg(long, value_enum, default_value_t = Corner::Center)]
        corner: Corner,
    },
    /// Waits until WIDGET names a widget, or with `--absent` none, and prints what it names.
    WaitFor {
        /// A CSS selector, as `[text="Saved"]`, or an id `tree` printed.
        widget: String,
        /// Wait until it names none.
        #[arg(long)]
        absent: bool,
        /// How long to wait, in milliseconds.
        #[arg(long, default_value_t = 5000)]
        timeout: u64,
    },
    /// The text the one widget WIDGET names shows, or the first text inside it.
    GetText {
        /// A CSS selector, as `AppBar Text`, or an id `tree` printed.
        widget: String,
    },
    /// The layout of the one widget WIDGET names and of its children: each one's size,
    /// constraints and place in its parent, as DevTools' layout explorer shows them.
    Layout {
        /// A CSS selector, as `AppBar`, or an id `tree` printed.
        widget: String,
        /// The inspector's JSON, as a tool reads it.
        #[arg(long)]
        json: bool,
    },
    /// Whether the app answers, as flutter_driver's `checkHealth`.
    Health,
    /// Any service extension, an app's own included, as `call ext.notes.openNoteCount`.
    Call {
        /// The extension's whole name.
        extension: String,
        /// Its parameters, each `name=value`.
        params: Vec<String>,
    },
}

/// A point of a widget's box, as flutter_driver's `OffsetType` names it.
#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}

impl Corner {
    /// flutter_driver's name for it.
    fn offset_type(self) -> &'static str {
        match self {
            Corner::TopLeft => "topLeft",
            Corner::TopRight => "topRight",
            Corner::BottomLeft => "bottomLeft",
            Corner::BottomRight => "bottomRight",
            Corner::Center => "center",
        }
    }
}

/// The trees `dump` prints.
#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum DumpedTree {
    /// The element tree, each widget with its properties: Flutter's `debugDumpApp`.
    App,
    /// The render tree: each object's constraints, size and properties.
    Render,
    /// The layer tree the frame is composited from.
    Layer,
    /// The focus tree, and which node has focus.
    Focus,
}

impl DumpedTree {
    fn extension(self) -> &'static str {
        match self {
            DumpedTree::App => "ext.waft.debugDumpApp",
            DumpedTree::Render => "ext.waft.debugDumpRenderTree",
            DumpedTree::Layer => "ext.waft.debugDumpLayerTree",
            DumpedTree::Focus => "ext.waft.debugDumpFocusTree",
        }
    }
}

/// Runs `command` on the app `app` names (by name, identifier or process id), or on the only one
/// running.
pub(crate) fn run(app: Option<&str>, command: ExtCommand) -> Result<()> {
    match command {
        ExtCommand::List => list(),
        ExtCommand::Dump { tree } => {
            let answer = Connected::to(app)?.call(tree.extension(), BTreeMap::new())?;
            print!("{}", answer["data"].as_str().unwrap_or_default());
            Ok(())
        }
        ExtCommand::Tree {
            id,
            depth,
            all,
            json,
        } => tree(app, id, depth, all, json),
        ExtCommand::Screenshot {
            id,
            view,
            scale,
            out,
        } => screenshot(app, id, view, scale, out),
        ExtCommand::LongPress {
            widget,
            unsynchronized,
        } => drive::long_press(app, &widget, unsynchronized),
        ExtCommand::TextInputAction {
            action,
            unsynchronized,
        } => drive::text_input_action(app, &action, unsynchronized),
        ExtCommand::ScrollIntoView { widget, alignment } => {
            drive::scroll_into_view(app, &widget, alignment)
        }
        ExtCommand::ScrollUntilVisible {
            item,
            scrollable,
            dx,
            dy,
            alignment,
            timeout,
        } => drive::scroll_until_visible(app, &item, &scrollable, (dx, dy), alignment, timeout),
        ExtCommand::Layout { widget, json } => drive::layout(app, &widget, json),
        ExtCommand::Offset { widget, corner } => drive::offset(app, &widget, corner.offset_type()),
        ExtCommand::Health => drive::health(app),
        ExtCommand::Find { widget } => drive::find(app, &widget),
        ExtCommand::Tap {
            widget,
            unsynchronized,
        } => drive::tap(app, &widget, unsynchronized),
        ExtCommand::EnterText {
            first,
            text,
            unsynchronized,
        } => match text {
            Some(text) => drive::enter_text(app, Some(&first), &text, unsynchronized),
            None => drive::enter_text(app, None, &first, unsynchronized),
        },
        ExtCommand::Scroll {
            widget,
            dx,
            dy,
            duration,
            unsynchronized,
        } => drive::scroll(app, &widget, (dx, dy), duration, unsynchronized),
        ExtCommand::WaitFor {
            widget,
            absent,
            timeout,
        } => drive::wait_for(app, &widget, absent, timeout),
        ExtCommand::GetText { widget } => drive::get_text(app, &widget),
        ExtCommand::Call { extension, params } => call(app, &extension, &params),
    }
}

/// Prints the apps running, one a line.
fn list() -> Result<()> {
    let apps = running();
    if apps.is_empty() {
        println!("No Waft app is running in a debug build on this machine.");
    }
    for (listed, _) in apps {
        let identifier = listed.identifier.as_deref().unwrap_or("-");
        let device = listed.device.as_deref().unwrap_or("this machine");
        println!(
            "{}  {identifier}  {device}  (process {})",
            listed.app, listed.pid
        );
    }
    Ok(())
}

/// Prints the tree from the root, as Flutter's tools ask for it: the summary tree of the widgets
/// the project made (every widget with `all`), each paragraph's text previewed; or, from the
/// widget `id`, its details `depth` levels down, properties included.
fn tree(app: Option<&str>, id: Option<String>, depth: u32, all: bool, json: bool) -> Result<()> {
    let mut connected = Connected::to(app)?;
    let tree = match id {
        None => {
            let params = BTreeMap::from([
                ("groupName".to_owned(), "cli".to_owned()),
                ("isSummaryTree".to_owned(), (!all).to_string()),
                ("withPreviews".to_owned(), "true".to_owned()),
            ]);
            connected.call("ext.waft.inspector.getRootWidgetTree", params)?
        }
        Some(id) => {
            let params = BTreeMap::from([
                ("arg".to_owned(), id.clone()),
                ("objectGroup".to_owned(), "cli".to_owned()),
                ("subtreeDepth".to_owned(), depth.to_string()),
            ]);
            let details = connected.call("ext.waft.inspector.getDetailsSubtree", params)?;
            if details["result"].is_null() {
                bail!("{id} names no widget now; `cargo waft ext tree` gives the ids of this run");
            }
            details
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&tree["result"])?);
    } else {
        print!("{}", outline::outline(&tree["result"]));
    }
    Ok(())
}

fn screenshot(
    app: Option<&str>,
    id: Option<String>,
    view: Option<u64>,
    scale: Option<f64>,
    out: Option<PathBuf>,
) -> Result<()> {
    let mut connected = Connected::to(app)?;
    let png = match id {
        Some(id) => {
            let params = BTreeMap::from([
                ("id".to_owned(), id.clone()),
                ("width".to_owned(), "100000".to_owned()),
                ("height".to_owned(), "100000".to_owned()),
                ("maxPixelRatio".to_owned(), scale.unwrap_or(2.0).to_string()),
            ]);
            let answer = connected.call("ext.waft.inspector.screenshot", params)?;
            let Some(png) = png_of(&answer) else {
                bail!("{id} cannot be pictured now: it is gone, not laid out, or paints nothing");
            };
            png
        }
        // A whole view, in its own pixels, as flutter_driver's `screenshot` pictures one.
        None => {
            let mut params = BTreeMap::from([("command".to_owned(), "screenshot".to_owned())]);
            if let Some(view) = view {
                params.insert("view".to_owned(), view.to_string());
            }
            let answer = connected.call("ext.waft.driver", params)?;
            png_of(&answer).context("the app answered no PNG")?
        }
    };
    write_screenshot(&png, out)
}

/// Writes `png` to `out`, or to the first of `waft_01.png`, `waft_02.png`, … not yet in this
/// folder, as `flutter screenshot` does, and says where: never to standard output, where a PNG
/// would flood a terminal or an agent's context.
fn write_screenshot(png: &[u8], out: Option<PathBuf>) -> Result<()> {
    let path = match out {
        Some(path) => path,
        None => unused_file("waft", "png")?,
    };
    std::fs::write(&path, png).with_context(|| format!("writing {}", path.display()))?;
    let kilobytes = png.len().div_ceil(1024);
    println!("Screenshot written to {} ({kilobytes}kB).", path.display());
    Ok(())
}

/// The first of `<base>_01.<extension>`, `<base>_02.<extension>`, … not in this folder: Flutter
/// tool's `getUniqueFile`.
fn unused_file(base: &str, extension: &str) -> Result<PathBuf> {
    (1..=999)
        .map(|number| PathBuf::from(format!("{base}_{number:02}.{extension}")))
        .find(|path| !path.exists())
        .context("every file name from waft_01.png to waft_999.png is taken here")
}

/// Calls `extension` with `params` (`name=value` each) and prints its answer: a dump's text as it
/// is, a screenshot written to a file as `screenshot` writes one, anything else as JSON.
fn call(app: Option<&str>, extension: &str, params: &[String]) -> Result<()> {
    let params = params
        .iter()
        .map(|param| {
            param
                .split_once('=')
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .with_context(|| format!("a parameter is `name=value`, not `{param}`"))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let answer = Connected::to(app)?.call(extension, params)?;
    match png_of(&answer) {
        Some(png) => write_screenshot(&png, None),
        None => {
            print!("{}", answer_text(&answer));
            Ok(())
        }
    }
}

/// A running app, connected, once it has drawn its first frame.
struct Connected {
    listed: ListedApp,
    client: Client,
}

impl Connected {
    fn to(app: Option<&str>) -> Result<Connected> {
        let (listed, client) = chosen(app)?;
        let mut connected = Connected { listed, client };
        connected.wait_for_the_first_frame()?;
        Ok(connected)
    }

    /// `extension`'s answer, or why it gave none: where the app stopped while answering, as when
    /// it crashed, that it stopped.
    fn call(&mut self, extension: &str, params: BTreeMap<String, String>) -> Result<Value> {
        let answer = self.client.call(extension, params);
        let name = format!("{} (process {})", self.listed.app, self.listed.pid);
        match answer {
            Ok(Ok(answer)) => Ok(answer),
            _ if !self.still_answers() => bail!(
                "{name} stopped while answering; its own output says why, as a crash's message"
            ),
            Ok(Err(error)) => bail!("{} answered: {error}", self.listed.app),
            Err(error) => Err(error).with_context(|| format!("calling {name}")),
        }
    }

    /// Whether the app still answers a new connection.
    fn still_answers(&self) -> bool {
        Client::connect(&self.listed)
            .and_then(|mut client| client.call(FIRST_FRAME, BTreeMap::new()))
            .is_ok()
    }

    /// Waits until the app has drawn its first frame, as Flutter's tools wait before they call
    /// an app: an app just started answers a dump before its tree is built, and may not have
    /// registered its extensions yet, which its tools wait for. An app that does not say, or has
    /// not drawn one in ten seconds, is called without waiting longer; one that does not answer
    /// at all is not called.
    fn wait_for_the_first_frame(&mut self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            match self.client.call(FIRST_FRAME, BTreeMap::new()) {
                Ok(Ok(answer)) if answer["enabled"] == "false" => {}
                Ok(Err(error)) if error.code == METHOD_NOT_FOUND => {}
                Ok(_) => return Ok(()),
                Err(error) => bail!(
                    "{} (process {}) does not answer: {error}; it may have ended",
                    self.listed.app,
                    self.listed.pid
                ),
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    }
}

/// The extension that says whether the app has drawn its first frame, as Flutter's does.
const FIRST_FRAME: &str = "ext.waft.didSendFirstFrameEvent";

/// The PNG a screenshot answers, `{"result": base64}`, decoded.
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
            "{}\n",
            serde_json::to_string_pretty(result).unwrap_or_default()
        ),
    }
}

/// The app `app` names, by its name, its identifier or its process id, or the only one running,
/// connected.
fn chosen(app: Option<&str>) -> Result<(ListedApp, Client)> {
    let mut apps = answering(|listed| app.is_none_or(|wanted| names(listed, wanted)));
    match apps.len() {
        1 => Ok(apps.remove(0)),
        0 => match app {
            Some(wanted) => {
                bail!("no running Waft app is named `{wanted}`; `cargo waft ext list` lists them")
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

/// Whether `wanted` names `listed`: its name, its identifier or its process id.
fn names(listed: &ListedApp, wanted: &str) -> bool {
    listed.app == wanted
        || listed.identifier.as_deref() == Some(wanted)
        || listed.pid.to_string() == wanted
}

/// The apps listed that answer, each connected; the folder's listings of those that do not are
/// removed.
fn running() -> Vec<(ListedApp, Client)> {
    answering(|_| true)
}

/// The apps listed that `wanted` keeps and that answer, each connected: only those are
/// connected to, so naming one app does not reach the others. The folder's listings of those
/// that do not answer are removed.
fn answering(wanted: impl Fn(&ListedApp) -> bool) -> Vec<(ListedApp, Client)> {
    let folder = ListingFolder::new();
    discover(&[&folder, &WaosPrograms])
        .into_iter()
        .filter(|listed| wanted(listed))
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
    use waft_service_protocol::Address;

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

    #[test]
    fn an_app_is_named_by_its_name_its_identifier_or_its_process() {
        let address = Address::Tcp("127.0.0.1:9000".parse().unwrap());
        let waos = ListedApp::new("waos", Some("dev.waos.launcher".to_owned()), 92715, address);
        assert!(names(&waos, "waos"));
        assert!(names(&waos, "dev.waos.launcher"));
        assert!(names(&waos, "92715"));
        assert!(!names(&waos, "notes"));
    }
}
