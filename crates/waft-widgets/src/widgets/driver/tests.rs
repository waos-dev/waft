use std::cell::Cell;
use std::future::Future;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use serde_json::Value;
use waft_embedder::{ServiceExtensionError, ServiceExtensionParams};
use waft_foundation::{App, Listener, ServiceExtensions, Task, ValueKey};
use waft_painting::TextDirection;

use crate::testing::WidgetTester;
use crate::widgets::widget_inspector::inspector_id;
use crate::{Column, Directionality, GestureDetector, IntoWidget, Row, SizedBox, Text, WidgetRef};

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
