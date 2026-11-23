use std::cell::Cell;
use std::future::Future;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use serde_json::Value;
use waft_embedder::{ServiceExtensionError, ServiceExtensionParams};
use waft_foundation::{App, Listener, ServiceExtensions, Task, ValueKey};
use waft_painting::TextDirection;
use waft_scheduler::{FrameCallback, Ticker};

use crate::testing::WidgetTester;
use crate::testing::{TestTextField, TestWidgetsApp};
use crate::widgets::widget_inspector::inspector_id;
use crate::{
    Column, Directionality, GestureDetector, IntoWidget, ListView, MediaQuery, MediaQueryData, Row,
    ScrollController, ScrollControllerLeaf, SizedBox, Text, WidgetRef,
};

/// A settings page: a row for each setting, its label and a keyed box standing for its switch.
fn settings() -> WidgetRef {
    let row = |label: &str, key: ValueKey<String>| {
        Row::new()
            .children(vec![
                Text::new(label).into_widget(),
                SizedBox::new().width(40.0).key(key.into()).into_widget(),
            ])
            .into_widget()
    };
    Directionality::new(
        TextDirection::Ltr,
        Column::new().children(vec![
            row("Wi-Fi", ValueKey::new("wifi".to_owned())),
            row("Bluetooth", ValueKey::new("bluetooth".to_owned())),
        ]),
    )
    .into_widget()
}

/// `command` of the driver with `params`, answered at once.
fn driver(app: &mut App, params: &[(&str, &str)]) -> Result<Value, ServiceExtensionError> {
    let params = ServiceExtensionParams::new(
        params
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned())),
    );
    let mut task = ServiceExtensions::call(app, "ext.waft.driver", params);
    match std::pin::Pin::new(&mut task).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(answer) => answer,
        Poll::Pending => panic!("the driver answers at once"),
    }
}

/// The descriptions of what `find` found for `selector`.
fn found(app: &mut App, selector: &str) -> Vec<String> {
    let answer = driver(app, &[("command", "find"), ("selector", selector)]).expect("found");
    answer["found"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|node| node["description"].as_str().unwrap_or("?").to_owned())
        .collect()
}

/// A widget is named by its type, its key, its text, its properties and where it is in the
/// tree, as CSS names a page's elements.
#[test]
fn a_selector_names_widgets_as_css_names_elements() {
    let mut tester = WidgetTester::new();
    tester.pump_widget(settings());
    tester.update(|app| {
        assert_eq!(found(app, "Text"), ["Text", "Text"]);
        assert_eq!(found(app, "#bluetooth"), ["SizedBox-[<'bluetooth'>]"]);
        assert_eq!(found(app, r#"[text="Wi-Fi"]"#), ["Text"]);
        assert_eq!(found(app, "[width=\"40.0\"]").len(), 2);
        assert_eq!(
            found(app, r#"Row:has([text="Wi-Fi"]) SizedBox"#),
            ["SizedBox-[<'wifi'>]"],
            "the box in the Wi-Fi row"
        );
        assert_eq!(
            found(app, "Column > Row:nth-child(2) SizedBox"),
            ["SizedBox-[<'bluetooth'>]"]
        );
        assert_eq!(found(app, r#"Row:not(:has([text^=Wi])) Text"#).len(), 1);
        assert_eq!(
            found(app, ".selected"),
            Vec::<String>::new(),
            "widgets have no classes"
        );
    });
}

/// The id `tree` printed names one widget, and an id that names none finds nothing.
#[test]
fn an_id_names_the_widget_tree_printed() {
    let mut tester = WidgetTester::new();
    tester.pump_widget(settings());
    tester.update(|app| {
        let answer = driver(app, &[("command", "find"), ("selector", "#wifi")]).unwrap();
        let id = answer["found"][0]["valueId"]
            .as_str()
            .expect("an id")
            .to_owned();
        let by_id = driver(app, &[("command", "find"), ("id", &id)]).unwrap();
        assert_eq!(by_id["found"][0]["description"], "SizedBox-[<'wifi'>]");
        let gone = driver(
            app,
            &[("command", "find"), ("id", &inspector_id_of_nothing())],
        );
        assert_eq!(gone.unwrap()["found"], serde_json::json!([]));
    });
}

fn inspector_id_of_nothing() -> String {
    inspector_id(waft_foundation::HandleId::from_bits(1))
}

/// What is not a selector, or not a command, is refused with why.
#[test]
fn what_the_driver_cannot_read_is_refused() {
    let mut tester = WidgetTester::new();
    tester.pump_widget(settings());
    tester.update(|app| {
        for refused in [
            &[("command", "find"), ("selector", "Text::before")][..],
            &[("command", "find"), ("selector", ":hover")],
            &[("command", "find")],
            &[("command", "fly"), ("selector", "Text")],
        ] {
            assert!(
                matches!(
                    driver(app, refused),
                    Err(ServiceExtensionError::InvalidParams(_))
                ),
                "{refused:?}"
            );
        }
    });
}

/// The answer of the driver `command` with `params`, the app pumped a frame at a time until it
/// comes, as a running app's frames come.
fn driven(
    tester: &mut WidgetTester,
    params: &[(&str, &str)],
) -> Result<Value, ServiceExtensionError> {
    let params = ServiceExtensionParams::new(
        params
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned())),
    );
    let mut task: Task<_> =
        tester.update(|app| ServiceExtensions::call(app, "ext.waft.driver", params));
    for _ in 0..1000 {
        if let Poll::Ready(answer) =
            std::pin::Pin::new(&mut task).poll(&mut Context::from_waker(Waker::noop()))
        {
            return answer;
        }
        tester.pump_by(Duration::from_millis(16));
    }
    panic!("the driver never answered");
}

/// Two buttons, each counting its taps.
fn buttons(taps: &[Rc<Cell<u32>>; 2]) -> WidgetRef {
    let button = |label: &str, taps: &Rc<Cell<u32>>| {
        let taps = Rc::clone(taps);
        GestureDetector::new()
            .on_tap(Listener::new(move |_app| taps.set(taps.get() + 1)))
            .child(
                SizedBox::new()
                    .width(100.0)
                    .height(40.0)
                    .child(Text::new(label)),
            )
            .into_widget()
    };
    Directionality::new(
        TextDirection::Ltr,
        Column::new().children(vec![button("Save", &taps[0]), button("Cancel", &taps[1])]),
    )
    .into_widget()
}

/// A tap lands on the widget named, as a touch does, once the app has settled, and answers
/// what it tapped.
#[test]
fn a_tap_lands_on_the_widget_named() {
    let taps = [Rc::new(Cell::new(0)), Rc::new(Cell::new(0))];
    let mut tester = WidgetTester::new();
    tester.pump_widget(buttons(&taps));
    let answer = driven(
        &mut tester,
        &[
            ("command", "tap"),
            ("selector", r#"GestureDetector:has([text="Cancel"])"#),
        ],
    )
    .expect("tapped");
    assert_eq!((taps[0].get(), taps[1].get()), (0, 1));
    assert_eq!(answer["tapped"]["description"], "GestureDetector");
    assert_eq!(answer["settled"], true);

    driven(
        &mut tester,
        &[("command", "tap"), ("selector", r#"[text="Save"]"#)],
    )
    .expect("tapped");
    assert_eq!(
        (taps[0].get(), taps[1].get()),
        (1, 1),
        "a tap on the label reaches its button"
    );
}

/// A command that acts on one widget refuses a selector naming several, listing each with its
/// id to choose by, and one naming none.
#[test]
fn a_tap_needs_one_widget() {
    let taps = [Rc::new(Cell::new(0)), Rc::new(Cell::new(0))];
    let mut tester = WidgetTester::new();
    tester.pump_widget(buttons(&taps));
    let Err(ServiceExtensionError::Failed(why)) = driven(
        &mut tester,
        &[("command", "tap"), ("selector", "GestureDetector")],
    ) else {
        panic!("refused");
    };
    assert!(
        why.starts_with("`GestureDetector` names 2 widgets"),
        "{why}"
    );
    assert_eq!(why.matches("inspector-").count(), 2, "{why}");
    let none = driven(&mut tester, &[("command", "tap"), ("selector", "Switch")]);
    assert!(
        matches!(none, Err(ServiceExtensionError::Failed(why)) if why.contains("names no widget"))
    );
    assert_eq!((taps[0].get(), taps[1].get()), (0, 0));
}

/// `child` as a test's view shows it: sized as the tester's view, its text left to right.
fn in_view<K>(child: impl IntoWidget<K>) -> WidgetRef {
    let sized: WidgetRef =
        MediaQuery::new(MediaQueryData::new().size(WidgetTester::SIZE), child).into_widget();
    Directionality::new(TextDirection::Ltr, sized).into_widget()
}

/// Typing goes into the focused field as the platform's text input sends it; naming the field
/// focuses it first, and with no field focused nothing is typed.
#[test]
fn text_is_entered_into_the_field_named() {
    let mut tester = WidgetTester::new();
    tester.pump_widget(TestWidgetsApp::new().home(TestTextField::new()));
    let unfocused = driven(&mut tester, &[("command", "enter_text"), ("text", "milk")]);
    assert!(
        matches!(unfocused, Err(ServiceExtensionError::Failed(why)) if why.contains("no text field has the focus"))
    );
    let entered = driven(
        &mut tester,
        &[
            ("command", "enter_text"),
            ("selector", "EditableText"),
            ("text", "groceries"),
        ],
    )
    .expect("entered");
    assert_eq!(entered["entered"], "groceries");
    let read = driven(
        &mut tester,
        &[("command", "get_text"), ("selector", "EditableText")],
    );
    assert_eq!(read.expect("read")["text"], "groceries");
}

/// A scroll drags the widget named by the offset given, over its duration, as a finger does.
#[test]
fn a_scroll_drags_the_list_named() {
    let mut tester = WidgetTester::new();
    let controller = tester.update(ScrollController::default);
    let items: Vec<WidgetRef> = (0..40)
        .map(|_| SizedBox::new().height(50.0).into_widget())
        .collect();
    tester.pump_widget(in_view(
        ListView::new()
            .controller(controller.erase())
            .children(items),
    ));
    let answer = driven(
        &mut tester,
        &[
            ("command", "scroll"),
            ("selector", "ListView"),
            ("dy", "-400"),
            ("duration", "300"),
        ],
    )
    .expect("scrolled");
    assert_eq!(answer["scrolled"]["description"], "ListView");
    let offset = controller.offset(&tester.cell.borrow());
    assert!(
        offset >= 380.0,
        "scrolled by the drag, then by its fling: {offset}"
    );
}

/// Waiting answers what the selector names once it does, and refuses after its timeout.
#[test]
fn waiting_ends_when_the_widget_comes_or_the_time_does() {
    let mut tester = WidgetTester::new();
    tester.pump_widget(settings());
    let found = driven(
        &mut tester,
        &[("command", "wait_for"), ("selector", "#wifi")],
    );
    assert_eq!(
        found.expect("found")["found"][0]["description"],
        "SizedBox-[<'wifi'>]"
    );
    let still = driven(
        &mut tester,
        &[
            ("command", "wait_for"),
            ("selector", "#wifi"),
            ("absent", "true"),
            ("timeout", "100"),
        ],
    );
    assert!(
        matches!(still, Err(ServiceExtensionError::Failed(why)) if why == "`#wifi` still names a widget after 100 ms")
    );
}

/// The text a widget shows is read from it, or from the first text inside it.
#[test]
fn the_text_a_widget_shows_is_read() {
    let mut tester = WidgetTester::new();
    tester.pump_widget(settings());
    let read = driven(
        &mut tester,
        &[("command", "get_text"), ("selector", "Row:nth-child(2)")],
    );
    assert_eq!(read.expect("read")["text"], "Bluetooth");
    let none = driven(
        &mut tester,
        &[("command", "get_text"), ("selector", "#wifi")],
    );
    assert!(
        matches!(none, Err(ServiceExtensionError::Failed(why)) if why.contains("shows no text"))
    );
}

/// On a screen that never settles, as one with a spinner, a command fails rather than act on a
/// moving screen, as flutter_driver's does; with frame sync off it acts at once.
#[test]
fn a_command_on_a_screen_that_never_settles_fails_unless_unsynchronized() {
    let taps = [Rc::new(Cell::new(0)), Rc::new(Cell::new(0))];
    let mut tester = WidgetTester::new();
    tester.pump_widget(buttons(&taps));
    let view = waft_embedder::View::id(&**tester.view());
    let _spinner = tester.update(|app| {
        let ticker = Ticker::new(app, FrameCallback::new(|_app, _elapsed| {}), Some(view));
        ticker.start(app);
        ticker
    });
    let save = [("command", "tap"), ("selector", r#"[text="Save"]"#)];
    let Err(ServiceExtensionError::Failed(why)) = driven(&mut tester, &save) else {
        panic!("refused");
    };
    assert!(
        why.starts_with("the app did not settle within 5 seconds"),
        "{why}"
    );
    assert_eq!(taps[0].get(), 0, "nothing tapped");

    let unsynchronized = [save[0], save[1], ("frameSync", "false")];
    let answer = driven(&mut tester, &unsynchronized).expect("tapped");
    assert_eq!(taps[0].get(), 1);
    assert_eq!(answer["settled"], false, "still spinning after the tap");
}

/// The usability test's sequence on a bouncing list with 60 pixels to scroll: up past its end,
/// up again at its end, then down past its start; each scroll answers, and the list comes back
/// within its range.
#[test]
fn scrolls_past_a_bouncing_lists_ends_answer_and_settle() {
    let mut tester = WidgetTester::new();
    let controller = tester.update(ScrollController::default);
    let rows = (WidgetTester::SIZE.height() + 60.0) / 50.0;
    let items: Vec<WidgetRef> = (0..rows as usize)
        .map(|_| SizedBox::new().height(50.0).into_widget())
        .chain(std::iter::once(SizedBox::new().height(10.0).into_widget()))
        .collect();
    tester.pump_widget(in_view(
        ListView::new()
            .controller(controller.erase())
            .physics(Rc::new(crate::BouncingScrollPhysics::new()))
            .children(items),
    ));
    for dy in ["-200", "-350", "200"] {
        let answer = driven(
            &mut tester,
            &[("command", "scroll"), ("selector", "ListView"), ("dy", dy)],
        );
        assert!(answer.is_ok(), "dy {dy}: {answer:?}");
    }
    let offset = controller.offset(&tester.cell.borrow());
    assert!(
        (0.0..=60.0).contains(&offset),
        "back within the range: {offset}"
    );
}

/// A tap waits for the widget it names to appear, as flutter_driver's `waitForElement` does.
#[test]
fn a_tap_waits_for_its_widget_to_appear() {
    let taps = [Rc::new(Cell::new(0)), Rc::new(Cell::new(0))];
    let mut tester = WidgetTester::new();
    tester.pump_widget(SizedBox::new());
    let view = Rc::clone(tester.view()) as waft_embedder::ViewRef;
    let later = buttons(&taps);
    tester.update(|app| {
        waft_foundation::Timer::new(
            app,
            Duration::from_millis(200),
            Listener::new(move |app| {
                let root = crate::view::View::new(Rc::clone(&view), later.clone()).into_widget();
                crate::WidgetsBinding::instance(app).attach_root_widget(app, root);
            }),
        );
    });
    driven(
        &mut tester,
        &[("command", "tap"), ("selector", r#"[text="Save"]"#)],
    )
    .expect("tapped");
    assert_eq!(taps[0].get(), 1);
}

/// A tap refuses a widget a touch at its centre would miss, saying what it lands on, as
/// flutter_test's `hitTestable` keeps such a widget out.
#[test]
fn a_tap_refuses_a_widget_a_touch_would_miss() {
    let taps = [Rc::new(Cell::new(0)), Rc::new(Cell::new(0))];
    let mut tester = WidgetTester::new();
    tester.pump_widget(
        crate::IgnorePointer::new()
            .ignoring(true)
            .child(buttons(&taps)),
    );
    let missed = driven(
        &mut tester,
        &[("command", "tap"), ("selector", r#"[text="Save"]"#)],
    );
    assert!(
        matches!(&missed, Err(ServiceExtensionError::Failed(why)) if why.contains("a touch at its centre lands on")),
        "{missed:?}"
    );
    assert_eq!(taps[0].get(), 0);
}

/// Offstage widgets are not in the tree a selector reads, `:has()` included; a text property is
/// read without its quotes; a whole number key is `[key=…]`.
#[test]
fn a_selector_reads_the_onstage_tree_and_plain_values() {
    let mut tester = WidgetTester::new();
    tester.pump_widget(Directionality::new(
        TextDirection::Ltr,
        Column::new().children(vec![
            crate::Offstage::new()
                .offstage(true)
                .child(Row::new().children(vec![Text::new("Wi-Fi").into_widget()]))
                .into_widget(),
            Row::new()
                .children(vec![
                    Text::new("Bluetooth").into_widget(),
                    SizedBox::new()
                        .key(ValueKey::new(7_i64).into())
                        .into_widget(),
                ])
                .into_widget(),
        ]),
    ));
    tester.update(|app| {
        assert_eq!(
            found(app, r#"Row:has([text="Wi-Fi"])"#),
            Vec::<String>::new()
        );
        assert_eq!(found(app, "Text[data=Bluetooth]"), ["Text"]);
        assert_eq!(found(app, r#"[key="7"]"#), ["SizedBox-[<7>]"]);
    });
}

/// A long press holds a touch past the long-press timeout, as flutter_test's `longPress`.
#[test]
fn a_long_press_reaches_the_widget_named() {
    let presses = Rc::new(Cell::new(0));
    let mut tester = WidgetTester::new();
    let counted = Rc::clone(&presses);
    tester.pump_widget(
        GestureDetector::new()
            .on_long_press(Listener::new(move |_app| counted.set(counted.get() + 1)))
            .child(crate::ColoredBox::new(waft_embedder::Color::new(
                0xFF00FF00,
            ))),
    );
    driven(
        &mut tester,
        &[("command", "long_press"), ("selector", "GestureDetector")],
    )
    .expect("pressed");
    assert_eq!(presses.get(), 1);
}

/// The keyboard's action key reaches the focused field, as `done` submits it.
#[test]
fn a_text_input_action_reaches_the_focused_field() {
    let submitted = Rc::new(std::cell::RefCell::new(None));
    let kept = Rc::clone(&submitted);
    let mut tester = WidgetTester::new();
    tester.pump_widget(TestWidgetsApp::new().home(
        TestTextField::new().on_submitted(move |_app, value| *kept.borrow_mut() = Some(value)),
    ));
    driven(
        &mut tester,
        &[
            ("command", "enter_text"),
            ("selector", "EditableText"),
            ("text", "milk"),
        ],
    )
    .expect("entered");
    driven(
        &mut tester,
        &[("command", "send_text_input_action"), ("action", "done")],
    )
    .expect("sent");
    assert_eq!(submitted.borrow().as_deref(), Some("milk"));
    let unknown = driven(
        &mut tester,
        &[("command", "send_text_input_action"), ("action", "fly")],
    );
    assert!(matches!(
        unknown,
        Err(ServiceExtensionError::InvalidParams(_))
    ));
}

/// A widget partly in a list's viewport is scrolled into view, as flutter_driver's
/// `scrollIntoView` reaches a widget its finder sees, one that is at least partly on screen.
#[test]
fn a_widget_is_scrolled_into_view() {
    let mut tester = WidgetTester::new();
    let controller = tester.update(ScrollController::default);
    let items: Vec<WidgetRef> = (0..40)
        .map(|item| {
            SizedBox::new()
                .height(70.0)
                .key(ValueKey::new(item as i64).into())
                .into_widget()
        })
        .collect();
    tester.pump_widget(in_view(
        ListView::new()
            .controller(controller.erase())
            .children(items),
    ));
    let before = controller.offset(&tester.cell.borrow());
    driven(
        &mut tester,
        &[
            ("command", "scroll_into_view"),
            ("selector", r#"[key="8"]"#),
        ],
    )
    .expect("shown");
    let after = controller.offset(&tester.cell.borrow());
    assert_eq!(before, 0.0);
    assert_eq!(
        after, 560.0,
        "item 8, from 560 to 630, at the start of the viewport"
    );
}

/// A screenshot pictures a whole view, the first when none is named, in its own pixels; the
/// app answers its health.
#[test]
fn a_view_is_pictured_whole_and_the_app_answers_its_health() {
    let mut tester = WidgetTester::with_platform(waft_test::TestPlatform::new().with_gpu());
    tester.pump_widget(settings());
    let health = driven(&mut tester, &[("command", "get_health")]).expect("healthy");
    assert_eq!(health["status"], "ok");
    let shot = driven(&mut tester, &[("command", "screenshot")]).expect("pictured");
    let png = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        shot["result"].as_str().expect("a PNG"),
    )
    .expect("base64");
    let size = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().unwrap());
    assert_eq!((size(16), size(20)), (2400, 1800));
    let unknown = driven(&mut tester, &[("command", "screenshot"), ("view", "99")]);
    assert!(
        matches!(unknown, Err(ServiceExtensionError::InvalidParams(why)) if why.contains("views are"))
    );
}
