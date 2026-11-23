# waft-widgets/src/testing
Flutter home: packages/flutter_test/lib/src
Ported against: ed2132410ee94b5a590cb7f67cee7a6ea9101a60

## Identical
- gestures.rs → test_pointer.dart, controller.dart (`tap`, `tapAt`, `fling`, `flingFrom` with `initialOffset`, `drag`, `dragFrom`, `longPress`, `longPressAt`, `moveBy`, `sendKeyEvent`, `sendKeyDownEvent`, `sendKeyUpEvent`, `nextPointer`)
- test_widgets_app.rs → test_widgets_app.dart (`TestWidgetsApp`)
- test_text_field.rs → packages/flutter/test/widgets/editable_text_tester.dart (`TestTextField`, `TestTextSelectionHandleControls`)
- text_input.rs → test_text_input.dart, widget_tester.dart (`TestTextInput`, `showKeyboard`, `enterText`)
- matchers.rs → matchers.dart (`findsNothing`, `findsOneWidget`, `findsWidgets`, `findsNWidgets`, `findsAtLeastNWidgets`)

## capture.rs → _matchers_io.dart (`captureImage`)

- Change: `captureImage(element)` is `capture_image(app, element)`, answering at once: the nearest repaint boundary's layer as a picture (`OffsetLayer::to_picture`) drawn by the platform (`Platform::picture_to_image_sync`), which is synchronous here.
  Reason: platform — the host draws a picture into an image at the call, so there is nothing to wait for.
  Affect: `None` where the platform has no renderer, as a test platform until given a GPU (`TestPlatform::with_gpu`); a test reads the image's pixels with `Platform::read_image_pixels` and encodes them with `waft_test::encode_png`. `matchesGoldenFile` and its `GoldenFileComparator` are not ported. Trigger: the golden tests the Material notes list.

## tester.rs → widget_tester.dart, binding.dart (`AutomatedTestWidgetsFlutterBinding`, `_TestWindowingOwner`)

- Change: `WidgetTester` owns a real `waft_shell::Shell` over its `TestPlatform`, so every frame (`frame`), view arrival/removal (`view_added`/`view_removed`), pointer report (`pointer_reports`), key (`key_data`), text input update/action, and event sent through `tester.shell()` (`pop_route`, `view_close_requested`, `view_focus_changed`, `app_lifecycle_state_changed`, `edit_command`, …) goes through the same `EmbedderClient` path a real host uses; `Shell::new` installs the platform's fonts (`TestFontSource` unless the test gives the platform others via `TestPlatform::with_system_fonts`).
  Reason: platform — Waft has one `EmbedderClient` (`Shell`) between any host and the framework, where flutter_test replaces the engine hooks on its test binding.
  Affect: a test sends host events through `tester.shell()` without wrapper methods, and pointer reports pass through `Shell`'s `PointerDataPacketConverter` as on a real device.

- Change: `WidgetTester::for_app(platform)` starts with no implicit view and no pre-test tree, for an app that bootstraps itself via `update(|app| run_app(..))` / `run_widget(..)`; on each `pump()` / `pump_by()`, windows requested through `Platform::request_view` (`TestPlatform::with_windows`) given unasked (`TestPlatform::give_view`), or added on a layer (`ViewEmbedder::add_view`) reach the app via `Shell::view_added`, and closed or removed views via `Shell::view_removed`.
  Reason: platform — Waft apps manage windows through `Platform::request_view` and `HostWindow::close` rather than Flutter's `WindowingOwner` / `WindowController`.
  Affect: multi-window apps run their real startup and window-creation paths in tests and see new or closed windows at the next pump, where flutter_test's `_TestWindowingOwner` creates stub window controllers directly.

- Change: `WidgetTester::view_of(&self, target)` answers the `TestView` the widget found is in, as Dart's `viewOf` answers its `TestFlutterView`, and `TestPointer` carries `view: ViewId` (set with `in_view(view)`, defaulting to `IMPLICIT_VIEW_ID`); the platform's views are `tester.platform().views()`, as Dart's are `tester.platformDispatcher.views`, and any `TestView` among them is `TestPlatform::test_view(id)`, the embedder's `View` being `Any`.
  Reason: platform — Waft names a view by its `ViewId` in `EmbedderClient` and `PointerReport`, where Dart passes the `FlutterView`.
  Affect: `tap`, `drag`, `long_press` and `fling` target the view of the widget found, and `tap_at`, `drag_from`, `long_press_at` and `fling_from` take a pointer id (`i64`) or a `TestPointer` with `in_view(view.id())`.

- Change: `WidgetTester::pump_until(what, done)` pumps tasks, view changes and frames until `done` returns `true`, sleeping between pumps until work on another thread wakes the app (`TestPlatform::wait_for_wake`), as a host's event loop does, and failing after 60 real seconds; it stands in for flutter_test's `runAsync` when waiting on work on other threads, and as `runAsync` does, leaves the app's clock where it is.
  Reason: language — Rust widget tests are synchronous rather than running inside a Dart `FakeAsync` zone that can escape into a root zone via `runAsync`.
  Affect: a test waiting on a background thread calls `pump_until` instead of `await tester.runAsync(...)`; what waits on a timer still needs `pump_by`.

- Change: `WidgetTester::pump()` and `pump_by(duration)` frame only the views that asked for a frame since the last pump, each frame of them at once, advancing `TestPlatform`'s fake clock together with the app's clock on `pump_by`.
  Reason: platform — a host frames the views that asked (`Platform::request_frame` names its view) and stamps frames with its clock, where flutter_test's binding has one `hasScheduledFrame` for the app.
  Affect: a pump after a change to one view's tree runs no frame of the others, and `Platform::now()` moves only as `pump_by` advances test time.

- Change: dropping a tester after a case that did not panic tears down whatever root widget was attached across every view and checks that no timer is pending; flutter_test's checks that debug variables are unset have no counterpart.
  Reason: language — each case has its own `App`, which holds its debug flags, clock and singletons, where flutter_test's isolate-global ones are reset and checked between tests.
  Affect: a case that leaves a flag set or a singleton changed cannot affect the next, and is not reported for it; a timer left pending in any window still fails teardown.

- Change: a view metric a case sets (`tester.view().set_physical_size(..)` and the rest, or on any of the platform's `TestView`s, given or made by `TestWindows`) reaches the framework at once, through a listener the tester installs on each `TestView`, as the shell's `view_metrics_changed` does: `RendererBinding::handle_metrics_changed`, then the checkpoint.
  Reason: platform — `TestFlutterView`'s setters call the dispatcher's `onMetricsChanged`; here the listener delivers the view's change while the caller borrows the view.
  Affect: a case sets a metric while it does not hold the app, and needs no `addTearDown(tester.view.reset)`, since each case has its own views.

- Change: a tester whose pump panicked (a case that catches the panic of a build, a layout or a paint) skips the end-of-test disposal and timer check when dropped.
  Reason: language — a panic unwinds out of the frame and leaves the tree half built, where Dart's framework reports the error through `FlutterError.reportError`, finishes the frame, and the case reads it with `takeException`.
  Affect: such a case is not checked for a timer left pending; the cases that expect a build to fail pump nothing after it.

## finder.rs → finders.dart, tree_traversal.dart

- Change: `find::text_containing` and `find::by_tooltip` take a string, matched by containment and by equality; Dart's take a `Pattern`, a `RegExp` among them.
  Reason: language — Rust has no `Pattern` type a string and a regular expression share, and the framework has no regular expressions.
  Affect: a case that finds by a regular expression matches its text with `find::by_widget_predicate`.

- Change: `find::descendant_or_self` and `find::ancestor_or_self` are Dart's `matchRoot: true`, and `Finder::skip_offstage(false)` is Dart's `skipOffstage: false`.
  Reason: language — Rust has no named optional arguments.
  Affect: none beyond the spelling.

- Change: a tester method that acts on one widget (`tap`, `widget`, `get_rect`, …) takes a `FindsOne` or `RenderTarget`: a `Finder`, which must find exactly one, or an element, a global key or a render box.
  Reason: language — Dart's `FinderBase<Element>` parameter is one type, and Rust has no overload by type for the element and render-box forms the older helpers pass.
  Affect: an older helper's element or render box still works where a finder would.

## entry.rs → widget_tester.dart (`testWidgets`, `TargetPlatformVariant`)

- Change: `test_widgets_variant(variant, body)` runs `body` once per platform inside one Rust test, stopping at the first platform that fails and naming it on stderr.
  Reason: language — Rust's test harness registers each test at compile time, so one function cannot add a test per variant value as `testWidgets(variant:)` does.
  Affect: a failure on one platform hides whether the later ones pass, and the test list shows one test, not one per platform.

## Deferred

- tester.rs: the end-of-test check for an exception the case did not take (`takeException`). Trigger: `FlutterError.reportError`.
- test_text_field.rs: `autofillHints`, `contextMenuBuilder`, `groupId` and `onChanged`. Trigger: a case that needs one.
- finder.rs: `find.bySemanticsLabel` and the semantics finders. Trigger: the semantics tree.
- finder.rs: `find.image`, `find.widgetWithImage`, `find.bySubtype`, `find.byElementType`, `find.backButton`, `find.closeButton` and `Finder.hitTestable`. Trigger: a case that needs one.
- finders.rs: the older eager helpers (`elements`, `ancestors`, `is_onstage`, `find_onstage_by_key`, `descendants_of_type`, `ancestor_of_type`, `center_of`, `global_rect`, `render_box_of`) and the cases that hold an `AnyElement` to hand them, which Dart writes as a finder (`find.descendant(of:)`, `find.ancestor(of:)`, `tester.renderObject(finder)`, `tester.getRect(finder)`). No `find::by_element` stands in: flutter_test has none, and a tester method already takes an element. Trigger: the next edit of each case's file.
