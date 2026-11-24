//! A `TextBox` whose caret is hidden (`show_cursor(false)`, Flutter's `TextField.showCursor`):
//! focused, it draws no caret and still takes what is typed.

use std::{rc::Rc, time::Duration};
use waft_embedder::{SystemFontSource, TargetPlatform, TextAffinity, TextSelection};
use waft_foundation::{App, AppCell};
use waft_painting::PaintingBinding;
use waft_rendering::RenderEditable;
use waft_scheduler::SchedulerBinding;
use waft_services::{TextEditingValue, TextInput};
use waft_test::{IMPLICIT_VIEW_ID, TestPlatform, TestView};
use waft_widgets::*;
use waft_winui::{AccentPalette, TextBox};

struct Fixture {
    cell: Rc<AppCell>,
    controller: TextEditingControllerRef,
    time: Duration,
}

impl Fixture {
    /// Mounts a focused, empty `TextBox`, as `configure` makes it, in an 800x600 Windows view.
    fn new(configure: impl FnOnce(TextBox) -> TextBox) -> Self {
        let platform = TestPlatform::new()
            .on(TargetPlatform::Windows)
            .with_view(Rc::new(TestView::new(800.0, 600.0)))
            .with_font_source(|| {
                Box::new(SystemFontSource::new(valo_system_fonts::fontmgr::platform()))
            });
        let cell = AppCell::with_platform(Rc::new(platform));
        let controller = {
            let mut app = cell.borrow_mut();
            PaintingBinding::instance(&mut app).install_platform_fonts(&mut app);
            let controller: TextEditingControllerRef =
                TextEditingController::new(&mut app).into_erased();
            let text_box = configure(TextBox::new(controller.clone()));
            let home = Overlay::wrap(
                Align::new()
                    .alignment(AlignmentGeometry::TOP_LEFT)
                    .child(SizedBox::new().width(300.0).child(text_box)),
            )
            .into_widget();
            let root = WidgetsApp::new(AccentPalette::default().base)
                .builder(move |_, _, _| home.clone())
                .into_widget();
            run_app(&mut app, root);
            controller
        };
        cell.elapse(Duration::ZERO);
        let mut fixture = Self {
            cell,
            controller,
            time: Duration::ZERO,
        };
        fixture.pump();
        let node = fixture.find(|app, element| {
            downcast_widget::<EditableText>(element.widget(app).as_ref())
                .map(|w| w.focus_node.clone())
        });
        node.request_focus(&mut fixture.cell.borrow_mut(), None);
        fixture.pump();
        fixture
    }

    /// Ten 20 ms frames.
    fn pump(&mut self) {
        let (cell, views) = (&self.cell, [IMPLICIT_VIEW_ID]);
        for _ in 0..10 {
            self.time += Duration::from_millis(20);
            SchedulerBinding::handle_begin_frame(&mut cell.borrow_mut(), &views, Some(self.time));
            cell.checkpoint();
            SchedulerBinding::handle_draw_frame(&mut cell.borrow_mut(), &views);
            cell.checkpoint();
        }
    }

    /// The host IME replacing the text with `text` and the caret at its end, as typing does.
    fn type_text(&mut self, text: &str) {
        let caret = text.encode_utf16().count() as i32;
        let value = TextEditingValue::new()
            .text(text)
            .selection(TextSelection::collapsed(caret, TextAffinity::Downstream));
        let mut app = self.cell.borrow_mut();
        TextInput::instance(&mut app).update_editing_value(&mut app, value);
        drop(app);
        self.cell.checkpoint();
        self.pump();
    }

    /// What `pick` gives for the one mounted element it gives anything for.
    fn find<T>(&self, pick: impl Fn(&App, AnyElement) -> Option<T>) -> T {
        let mut app = self.cell.borrow_mut();
        let mut stack = Vec::from_iter(WidgetsBinding::instance(&mut app).root_element(&app));
        while let Some(element) = stack.pop() {
            if let Some(found) = pick(&app, element) {
                return found;
            }
            stack.extend(element.children(&app));
        }
        panic!("no such element")
    }

    /// Whether the editor draws its caret now.
    fn draws_the_caret(&self) -> bool {
        self.find(|app, element| {
            let editable = element
                .render_object(app)?
                .downcast::<RenderEditable>(app)?;
            Some(*editable.show_cursor(app).value(app))
        })
    }

    fn text(&self) -> String {
        self.controller
            .text_value(&self.cell.borrow_mut())
            .to_owned()
    }
}

#[test]
fn a_focused_box_draws_its_caret_by_default() {
    let fixture = Fixture::new(|text_box| text_box);
    assert!(fixture.draws_the_caret());
}

#[test]
fn a_box_told_to_hide_its_caret_draws_none_and_still_takes_what_is_typed() {
    let mut fixture = Fixture::new(|text_box| text_box.show_cursor(false));
    assert!(!fixture.draws_the_caret(), "focused, with no caret");
    fixture.type_text("notes");
    assert_eq!(fixture.text(), "notes");
    assert!(
        !fixture.draws_the_caret(),
        "through the blink's next half too"
    );
}
