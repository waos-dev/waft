# waft-winui-test-support

Unpublished test support for this workspace. Add it only under a consuming package's `[dev-dependencies]`:

```toml
[dev-dependencies]
waft-winui-test-support.workspace = true
```

`Fixture::with_root(size, run)` takes a physical pixel size `[u32; 2]` and a setup callback `FnOnce(&mut App)` that calls `waft_widgets::run_app` to mount the test widget. The `mount` helper below creates that callback. The fixture can pump frames, send input, find text labels, and capture rendered output. Tests using it require an available GPU adapter. `capture` writes PNG files when `WINUI_CAPTURE_DIR` is set.

The fixture runs the app on `waft-test`'s `TestPlatform` and `TestView`, with the system's fonts and a GPU with no window (`with_system_fonts`, `with_gpu`). Pumping frames draws nothing, as Flutter's test binding composites without rasterizing; `pixels` and `capture` capture the app's `View` when asked (`waft_widgets::testing::capture_image`) and read it back (`Platform::read_image_pixels`). `pixels` lays what it reads over white, the window the WinUI tests are written against; `capture` writes it as read, clear where nothing is painted.

```rust,no_run
use waft_widgets::{IntoWidget, SizedBox};
use waft_winui_test_support::{Fixture, mount};

let fixture = Fixture::with_root(
    [400, 300],
    mount(|_| SizedBox::new().width(40.0).height(20.0).into_widget()),
);
fixture.capture("empty-box");
```

This crate does not depend on `waft-winui` or `winui_gallery`. Gallery startup, feature selection and page navigation remain in `examples/winui_gallery/tests/all/common.rs` as `GalleryFixtureExt`. Control tests can use `Fixture::with_root` directly without loading the gallery.
