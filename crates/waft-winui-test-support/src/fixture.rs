//! Reusable application fixture with simulated time and input.

use std::{rc::Rc, time::Duration};
use waft_embedder::valo::PixelBuffer;
use waft_embedder::{
    Offset, PointerChange, PointerData, PointerDataPacket, PointerDeviceKind, TargetPlatform, View,
};
use waft_foundation::{App, AppCell};
use waft_gestures::GestureBinding;
use waft_rendering::RenderParagraph;
use waft_scheduler::SchedulerBinding;
use waft_test::{MockClipboard, TestPlatform, TestView};
use waft_widgets::testing::{capture_image, find};
use waft_widgets::{AnyElement, WidgetRef, WidgetsBinding};

/// Mounts an arbitrary application with simulated time and pointer input.
pub struct Fixture {
    /// Application arena and scheduler checkpoints.
    pub cell: Rc<AppCell>,
    /// The view the app draws into, which draws its last frame when its pixels are read.
    pub view: Rc<TestView>,
    /// Simulated event timestamp.
    pub at: Duration,
    /// Gesture identifiers increase on each down, as in the engine converter.
    next_pointer: i64,
    /// Identifier of the current touch gesture, shared with manually constructed packets.
    pub touch_pointer: i64,
    /// Identifier of the current mouse gesture, shared with manually constructed packets.
    pub mouse_pointer: i64,
    /// The platform's clipboard, in memory.
    clipboard: MockClipboard,
}
impl Fixture {
    /// Mounts whatever `run` starts, for a test of one control.
    pub fn with_root(size: [u32; 2], run: impl FnOnce(&mut App)) -> Self {
        let view = Rc::new(TestView::new(size[0].into(), size[1].into()));
        let clipboard = MockClipboard::new();
        let platform = TestPlatform::new()
            .on(TargetPlatform::MacOS)
            .with_view(view.clone())
            .with_clipboard(Rc::new(clipboard.clone()))
            .with_system_fonts()
            .with_gpu();
        let cell = AppCell::with_platform(Rc::new(platform));
        {
            let mut app = cell.borrow_mut();
            // What the shell installs at start-up, which a fixture runs the app without.
            waft_painting::PaintingBinding::instance(&mut app).install_platform_fonts(&mut app);
            run(&mut app);
        }
        cell.elapse(Duration::ZERO);
        let mut result = Self {
            cell,
            view,
            at: Duration::ZERO,
            next_pointer: 0,
            touch_pointer: 0,
            mouse_pointer: 0,
            clipboard,
        };
        result.pump();
        result
    }
    /// Runs ten 20 ms refreshes of the view.
    pub fn pump(&mut self) {
        for _ in 0..10 {
            self.frame_after(Duration::from_millis(20));
        }
    }

    /// Runs one refresh of the view, `elapsed` after the last, processing microtasks between its
    /// phases.
    pub fn frame_after(&mut self, elapsed: Duration) {
        self.at += elapsed;
        let views = [self.view.id()];
        SchedulerBinding::handle_begin_frame(&mut self.cell.borrow_mut(), &views, Some(self.at));
        self.cell.checkpoint();
        SchedulerBinding::handle_draw_frame(&mut self.cell.borrow_mut(), &views);
        self.cell.checkpoint();
    }
    /// The text on the platform's clipboard.
    pub fn clipboard_text(&self) -> Option<String> {
        self.clipboard.clipboard_data()
    }

    /// Enumerates all mounted elements, including retained offstage subtrees.
    pub fn elements(&self) -> Vec<AnyElement> {
        let mut app = self.cell.borrow_mut();
        let root = WidgetsBinding::instance(&mut app)
            .root_element(&app)
            .unwrap();
        let mut all = vec![root];
        let mut i = 0;
        while i < all.len() {
            all.extend(all[i].children(&app));
            i += 1;
        }
        all
    }
    /// Enumerates painted subtrees while excluding descendants of Offstage widgets.
    fn onstage_elements(&self) -> Vec<AnyElement> {
        let mut app = self.cell.borrow_mut();
        let root = WidgetsBinding::instance(&mut app)
            .root_element(&app)
            .unwrap();
        let mut elements = vec![root];
        let mut index = 0;
        while index < elements.len() {
            let element = elements[index];
            let offstage = waft_widgets::downcast_widget::<waft_widgets::Offstage>(
                element.widget(&app).as_ref(),
            )
            .is_some_and(|widget| widget.offstage);
            if !offstage {
                elements.extend(element.children(&app));
            }
            index += 1;
        }
        elements
    }

    /// Finds the center of the first onstage text label.
    pub fn find(&self, text: &str) -> Offset {
        let elements = self.onstage_elements();
        let app = self.cell.borrow();
        for element in elements {
            if let Some(object) = element.render_object(&app)
                && let Some(paragraph) = object.downcast::<RenderParagraph>(&app)
                && paragraph.text(&app).to_plain_text(true, true) == text
            {
                let object = object.as_box().unwrap();
                let size = object.size(&app);
                return object.local_to_global(
                    &app,
                    Offset::new(size.width() / 2.0, size.height() / 2.0),
                    None,
                );
            }
        }
        panic!("missing label {text}")
    }
    /// Brings an already mounted label into view before a test sends pointer input.
    pub fn ensure_visible(&mut self, text: &str) {
        let elements = self.onstage_elements();
        let mut app = self.cell.borrow_mut();
        let context = elements
            .into_iter()
            .find(|element| {
                element
                    .render_object(&app)
                    .and_then(|object| object.downcast::<RenderParagraph>(&app))
                    .is_some_and(|paragraph| paragraph.text(&app).to_plain_text(true, true) == text)
            })
            .unwrap_or_else(|| panic!("missing label {text}"));
        drop(waft_widgets::Scrollable::ensure_visible(
            &mut app,
            context,
            0.5,
            Duration::ZERO,
            waft_animation::Curves::linear(),
            waft_widgets::ScrollPositionAlignmentPolicy::Explicit,
        ));
        drop(app);
        self.cell.checkpoint();
        self.pump();
    }

    /// Sends a touch event at the fixture timestamp.
    pub fn send(&mut self, change: PointerChange, point: Offset) {
        if change == PointerChange::Down {
            self.next_pointer += 1;
            self.touch_pointer = self.next_pointer;
        }
        let mut app = self.cell.borrow_mut();
        GestureBinding::instance(&mut app).handle_pointer_data_packet(
            &mut app,
            PointerDataPacket::new(vec![PointerData {
                change,
                kind: PointerDeviceKind::Touch,
                time_stamp: self.at,
                pointer_identifier: self.touch_pointer,
                physical_x: point.dx(),
                physical_y: point.dy(),
                ..Default::default()
            }]),
        );
        drop(app);
        self.cell.checkpoint();
    }
    /// A mouse event: hover moves have no buttons, a press and its moves carry the primary button.
    pub fn send_mouse(&mut self, change: PointerChange, point: Offset, buttons: i64) {
        if change == PointerChange::Down {
            self.next_pointer += 1;
            self.mouse_pointer = self.next_pointer;
        }
        let mut app = self.cell.borrow_mut();
        GestureBinding::instance(&mut app).handle_pointer_data_packet(
            &mut app,
            PointerDataPacket::new(vec![PointerData {
                change,
                kind: PointerDeviceKind::Mouse,
                time_stamp: self.at,
                pointer_identifier: self.mouse_pointer,
                physical_x: point.dx(),
                physical_y: point.dy(),
                buttons,
                ..Default::default()
            }]),
        );
        drop(app);
        self.cell.checkpoint();
    }

    /// Traverses with Tab presses until the named control owns keyboard focus.
    pub fn focus(&mut self, text: &str) {
        use waft_services::{
            HardwareKeyboard, KeyDownEvent, KeyEvent, KeyUpEvent, LogicalKeyboardKey,
            PhysicalKeyboardKey,
        };
        let elements = self.onstage_elements();
        let mut app = self.cell.borrow_mut();
        let context = elements
            .into_iter()
            .find(|element| {
                element
                    .render_object(&app)
                    .and_then(|object| object.downcast::<RenderParagraph>(&app))
                    .is_some_and(|paragraph| paragraph.text(&app).to_plain_text(true, true) == text)
            })
            .unwrap_or_else(|| panic!("missing label {text}"));
        let target = waft_widgets::Focus::of(&mut app, context, false, false);
        drop(app);
        for _ in 0..100 {
            if waft_widgets::primary_focus(&mut self.cell.borrow_mut()) == Some(target) {
                return;
            }
            for event in [
                KeyEvent::Down(KeyDownEvent::new(
                    PhysicalKeyboardKey::TAB,
                    LogicalKeyboardKey::TAB,
                    self.at,
                )),
                KeyEvent::Up(KeyUpEvent::new(
                    PhysicalKeyboardKey::TAB,
                    LogicalKeyboardKey::TAB,
                    self.at,
                )),
            ] {
                let mut app = self.cell.borrow_mut();
                HardwareKeyboard::instance(&mut app).handle_key_event(&mut app, &event);
                drop(app);
                self.cell.checkpoint();
            }
            self.pump();
        }
        panic!("Tab traversal did not reach {text}");
    }

    /// Taps a text label and pumps the resulting frames.
    pub fn tap(&mut self, text: &str) {
        let p = self.find(text);
        self.send(PointerChange::Down, p);
        self.at += Duration::from_millis(20);
        self.send(PointerChange::Up, p);
        self.pump();
    }
    /// What the view shows over a white window, the backdrop the WinUI tests are written
    /// against, as RGBA rows of [`size`](Self::size): its `View` captured (`capture_image`) and read back,
    /// clear where nothing is painted, then laid over white.
    pub fn pixels(&self) -> Vec<u8> {
        over_white(self.captured().into_bytes())
    }

    /// The view's size in physical pixels, the size of what [`pixels`](Self::pixels) captures.
    pub fn size(&self) -> [u32; 2] {
        self.view
            .metrics()
            .physical_size
            .map(|length| length.ceil() as u32)
    }

    /// Captures the frame, written under WINUI_CAPTURE_DIR as `name` where it is set.
    pub fn capture(&self, name: &str) {
        let pixels = self.captured();
        if let Ok(dir) = std::env::var("WINUI_CAPTURE_DIR") {
            std::fs::create_dir_all(&dir).expect("the capture folder is made");
            let path = std::path::Path::new(&dir).join(format!("{name}.png"));
            std::fs::write(path, waft_test::encode_png(&pixels)).expect("the capture is written");
        }
    }

    fn captured(&self) -> PixelBuffer {
        let mut app = self.cell.borrow_mut();
        let view = *find::by_type::<waft_widgets::View>()
            .evaluate_in(&mut app)
            .first()
            .expect("the app's view");
        let image = capture_image(&mut app, view).expect("the test platform draws images");
        let pixels = pollster::block_on(app.platform().read_image_pixels(&image))
            .expect("the frame reads back");
        assert_eq!(pixels.layout().size, self.size());
        pixels
    }
}

/// Straight-alpha RGBA `pixels` laid over opaque white, as a window's white backdrop shows them.
fn over_white(mut pixels: Vec<u8>) -> Vec<u8> {
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = u32::from(pixel[3]);
        for channel in &mut pixel[..3] {
            *channel = ((u32::from(*channel) * alpha + 255 * (255 - alpha) + 127) / 255) as u8;
        }
        pixel[3] = 255;
    }
    pixels
}

/// `waft_widgets::run_app` for a widget built once the app exists.
pub fn mount(root: impl FnOnce(&mut App) -> WidgetRef + 'static) -> impl FnOnce(&mut App) {
    move |app| {
        let widget = root(app);
        waft_widgets::run_app(app, widget);
    }
}
