//! flutter_test's keyboard for a case: [`WidgetTester::show_keyboard`],
//! [`WidgetTester::enter_text`] and [`WidgetTester::test_text_input`].
//!
//! A case that only types into a field calls [`enter_text`](WidgetTester::enter_text); a case
//! that tests the keyboard itself calls [`show_keyboard`](WidgetTester::show_keyboard) and then
//! [`TestTextInput`]'s methods, as it would call `testTextInput` in Dart. The text reaches the
//! client the app attached last, through [`TextInput`](waft_services::TextInput), as a
//! platform's message does.

use waft_embedder::{EmbedderClient, TextAffinity, TextInputAction};
use waft_services::{TextEditingValue, TextInput};

use super::find;
use super::{Finder, WidgetTester};

impl WidgetTester {
    /// Dart's `tester.showKeyboard(finder)`: gives the text input widget `finder` finds the
    /// focus, as if the onscreen keyboard had appeared, and pumps.
    ///
    /// The widget must be an [`EditableText`](crate::EditableText) or have one under it.
    pub fn show_keyboard(&mut self, finder: Finder) {
        let field = find::descendant_or_self(finder, find::by_type::<crate::EditableText>());
        let element = self.element(field);
        let state = element
            .state_handle::<crate::EditableTextState>(&self.cell.borrow())
            .expect("an EditableText state");
        state.request_keyboard(&mut self.cell.borrow_mut());
        self.pump();
    }

    /// Dart's `tester.enterText(finder, text)`: shows the keyboard for `finder` and replaces
    /// the field's content with `text`, the caret at its end.
    pub fn enter_text(&mut self, finder: Finder, text: &str) {
        self.show_keyboard(finder);
        self.test_text_input().enter_text(text);
    }

    /// Dart's `tester.testTextInput`: the stub keyboard the app's text input reaches.
    pub fn test_text_input(&mut self) -> TestTextInput<'_> {
        TestTextInput { tester: self }
    }
}

/// flutter_test's `TestTextInput`: the keyboard a case types on.
///
/// The methods send what a platform sends to the client the app attached last, through the
/// shell's text-input methods; a field that never asked for the keyboard drops them, as Dart's
/// stub does without a client.
pub struct TestTextInput<'a> {
    tester: &'a mut WidgetTester,
}

impl TestTextInput<'_> {
    /// Dart's `testTextInput.enterText(text)`: replaces the connected field's content with
    /// `text`, the caret at its end.
    pub fn enter_text(&mut self, text: &str) {
        let length = text.encode_utf16().count() as i32;
        self.update_editing_value(TextEditingValue::new().text(text).selection(
            waft_embedder::TextSelection::collapsed(length, TextAffinity::Downstream),
        ));
    }

    /// Dart's `testTextInput.updateEditingValue(value)`: the connected field takes `value`.
    pub fn update_editing_value(&mut self, value: TextEditingValue) {
        let view = self.tester.default_view_id();
        self.tester.shell().text_input_editing_value(view, value);
    }

    /// Dart's `testTextInput.receiveAction(action)`: the connected field performs `action`.
    pub fn receive_action(&mut self, action: TextInputAction) {
        let view = self.tester.default_view_id();
        self.tester.shell().text_input_action(view, action);
    }

    /// Dart's `testTextInput.hasAnyClients`: whether a field has the input connection.
    pub fn has_any_clients(&self) -> bool {
        let mut app = self.tester.cell.borrow_mut();
        let input = TextInput::instance(&mut app);
        input.current_connection(&app).is_some()
    }

    /// Dart's `testTextInput.editingState`: the last editing value the framework sent the
    /// host, or `None` before it sent one.
    pub fn editing_state(&self) -> Option<TextEditingValue> {
        self.tester
            .view()
            .test_host()
            .text_input_calls()
            .into_iter()
            .rev()
            .find_map(|call| match call {
                waft_test::TextInputCall::SetEditingState(value) => Some(value),
                _ => None,
            })
    }
}
