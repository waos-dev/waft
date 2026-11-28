<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/waft-logo-dark.png">
    <img src="assets/waft-logo.png" alt="Waft" width="96" height="96">
  </picture>
</p>

<h1 align="center">Waft</h1>

<p align="center">
  A pure Rust, GPU-accelerated UI framework for building fast, fluid apps.
</p>

Waft is a cross-platform UI framework in Rust targeting macOS, Windows, Linux, iOS, Android, and the web — with a clean embedder interface for bringing your own platform.

## Why Waft

- **Solid foundation:** Built on [Flutter](https://github.com/flutter/flutter)'s proven architecture, Waft ships with rich layout, gestures, animations, and ready to use widget kits from day one.
- **Pure Rust, easy to embed:** Everything including the [render engine](https://github.com/seedeai/valo) builds with a single `cargo` command, behind a clean embedder interface for custom platforms.
- **Fast & GPU-accelerated:** Compiles to a native binary with GPU acceleration — small footprint, fast rendering, and instant startup.
- **Cross-platform:** One codebase for macOS, Windows, Linux, iOS, Android, and the web, with an easy-to-use CLI to run and package for each target.
- **First-class desktop:** Built for real desktop apps with native menus, multi-window support, single-instance locking, and packaging for DMG, MSI, DEB, and AppImage.
- **Hardware compositing & media:** Video, camera previews, and cross-process surfaces run as native OS compositor layers with zero app repaints.
- **Built for coding agents:** Built-in debugging and UI-driving tools (`cargo waft ext`) so agents never need to leave the terminal ([waft.rs/agents.md](https://waft.rs/agents.md)).

## Apps built with Waft

<table>
  <tr>
    <td align="center" valign="bottom" width="25%">
      <a href="https://edged2.app">
        <img src="https://edged2.app/edged.png" alt="Edged 2" width="72" height="72">
      </a>
    </td>
    <td align="center" valign="bottom" width="25%">
      <a href="https://cupertino-gallery.waft.rs">
        <img src="assets/cupertino-gallery.png" alt="Cupertino gallery" width="140">
      </a>
    </td>
    <td align="center" valign="bottom" width="25%">
      <a href="https://winui-gallery.waft.rs">
        <img src="assets/winui-gallery.png" alt="WinUI gallery" width="180">
      </a>
    </td>
    <td align="center" valign="bottom" width="25%">
      <a href="https://material-gallery.waft.rs">
        <img src="assets/material-gallery.png" alt="Material gallery" width="180">
      </a>
    </td>
  </tr>
  <tr>
    <td align="center" valign="top">
      <a href="https://edged2.app"><strong>Edged 2</strong></a>
    </td>
    <td align="center" valign="top">
      <a href="https://cupertino-gallery.waft.rs"><strong>Cupertino gallery</strong></a>
    </td>
    <td align="center" valign="top">
      <a href="https://winui-gallery.waft.rs"><strong>WinUI gallery</strong></a>
    </td>
    <td align="center" valign="top">
      <a href="https://material-gallery.waft.rs"><strong>Material gallery</strong></a>
    </td>
  </tr>
  <tr>
    <td align="center" valign="top">
      A window switcher for macOS
    </td>
    <td align="center" valign="top">
      iOS-style widgets, from<br>
      <a href="https://github.com/waos-dev/waft/tree/main/crates/waft-cupertino">waft-cupertino</a>
    </td>
    <td align="center" valign="top">
      Desktop and web widgets, from<br>
      <a href="https://github.com/waos-dev/waft/tree/main/crates/waft-winui">waft-winui</a>
    </td>
    <td align="center" valign="top">
      Material Design 3 widgets, from<br>
      <a href="https://github.com/waos-dev/waft/tree/main/crates/waft-material">waft-material</a>
    </td>
  </tr>
</table>

## Start an app

`cargo waft` is a CLI for scaffolding, running, and packaging Waft apps:

```sh
cargo install waft-cli
cargo waft new myapp                  # --template winui for a desktop app
cd myapp
cargo waft run                        # this desktop
cargo waft run -d ios                 # an iOS simulator
cargo waft run -d android             # an Android emulator
cargo waft run -d web                 # a browser
cargo waft build dmg                  # and macos, ios, ipa, apk, msi, nsis, deb, appimage, web
```

> For more details, see [`waft-cli`](crates/waft-cli/README.md).

## Building an app with Waft

Waft keeps a clean interface between the framework and the platform (the embedder). The usual way to start an app is to create an embedder and provide a root widget:

```rust
use waft::{DefaultEmbedder, IntoWidget, Shell, run_app};
use waft_cupertino::CupertinoApp;

fn main() {
    DefaultEmbedder::default().run(|platform| {
        Shell::new(platform, |app| {
            run_app(app, CupertinoApp::new().home(Counter).into_widget());
        })
    });
}
```

Or use the `#[waft::main]` macro to write the main function:

```rust
use waft::{App, IntoWidget, run_app};
use waft_cupertino::CupertinoApp;

#[waft::main]
fn main(app: &mut App) {
    run_app(app, CupertinoApp::new().home(Counter).into_widget());
}
```

### Writing the UI

The framework is a faithful port of Flutter with a large number of widgets available. Use them as you would in Flutter:

```rust
use waft::{App, BuildContext, Column, IntoWidget, StatelessWidget, Text, WidgetRef};

#[derive(Debug)]
struct Counter;

impl StatelessWidget for Counter {
    fn build(&self, _app: &mut App, _context: BuildContext) -> WidgetRef {
        Column::new()
            .children([
                Text::new("Counter").into_widget(),
                Text::new("0").into_widget(),
            ])
            .into_widget()
    }
}
```

### State management

Unlike Flutter, Waft has a built-in state management solution, inspired by [Zed's GPUI](https://zed.dev/blog/gpui-ownership).

App state lives in an `Entity`. Create one with `app.new_entity`, read it with `read`, and change it with `update`. A `read` inside a widget's `build` observes the entity: `notify` rebuilds that widget.


```rust
use waft::{App, BuildContext, Entity, IntoWidget, StatelessWidget, Text, WidgetRef};

let count = app.new_entity(|_cx| 0);

struct Label {
    count: Entity<i32>,
}

impl StatelessWidget for Label {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let n = *app.read(&self.count);
        Text::new(format!("{n}")).into_widget()
    }
}

app.update(&count, |n, cx| {
    *n += 1;
    cx.notify();
});
```

### Structuring an app

The recommended architecture is three layers:

- **Logic**: the business logic, as plain Rust. It does not depend on `App` or `Entity`, so it runs and is tested on its own.
- **State**: the app's state as entities, built on the logic layer. This is where `App`, `Entity` and `Timer` are used. Entity events model what happens: an entity emits with `cx.emit`, another subscribes and reacts. A callback from outside the app reaches an entity through `AsyncApp::post`. This layer depends on `waft-foundation` alone, so it is tested with `AppCell` and no display.
- **UI**: widgets, kept thin. A widget reads entities in `build`, which subscribes it to them, and calls their methods from callbacks. It keeps only view state, such as whether a panel is open.

A new feature is one entity plus the widgets that show it.

## Contributing and AI

Waft is developed with strong assistance from AI agents, with humans leading the design, reviewing changes, testing, and remaining responsible for correctness.

AI-assisted contributions are welcome as long as the contributor fully reviews the work, understands how it fits the architecture, manually reviews code changes, and tests the work. Code that the contributor cannot explain, verify, or maintain should not be submitted.

How this repo is worked on: [AGENTS.md](AGENTS.md).

## License

MIT
