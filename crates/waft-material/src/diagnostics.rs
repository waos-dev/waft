//! Dart `toString` for property values this port holds without `Display`.
//!
//! A Flutter object kept in the arena prints as its `toStringShort`
//! (`FocusNode#1a2b3`); an enum whose Dart `toString` is typed (`Brightness.dark`) is
//! written from its `Debug` name; a `WidgetStateProperty` prints as Dart's
//! `WidgetStatePropertyAll(value)` when it is one. Each function returns the text a
//! `DiagnosticsProperty` shows, which is why the callers make the property a
//! [`DiagnosticsProperty`](waft_foundation::DiagnosticsProperty)`<String>`.

use std::fmt::Debug;

use waft_foundation::{HandleId, describe_enum, describe_identity};
use waft_widgets::{
    WidgetStateProperty, WidgetStatePropertyAll, WidgetStatePropertyRef, WidgetStates,
};

/// Dart's `toStringShort` of an object this port holds by arena id.
pub(crate) fn handle_identity(type_name: &str, id: HandleId) -> String {
    describe_identity(type_name, id.identity_hash())
}

/// Dart's `DiagnosticsProperty<Enum>` text for an enum that overrides `toString`: the type
/// and the enum's name, as `Brightness.dark`.
pub(crate) fn typed_enum(type_name: &str, value: &impl Debug) -> String {
    format!("{type_name}.{}", describe_enum(value))
}

/// Dart's `WidgetStatePropertyAll.toString`, or the property's own `Debug` for another
/// implementation. A Dart `T?` value is printed without Rust's `Some`, as Dart prints it.
pub(crate) fn widget_state_property<T: Clone + Debug + PartialEq + 'static>(
    property: &WidgetStatePropertyRef<Option<T>>,
) -> String {
    let property: &dyn WidgetStateProperty<Option<T>> = &**property;
    match property
        .as_any()
        .downcast_ref::<WidgetStatePropertyAll<Option<T>>>()
    {
        Some(all) => match &all.value {
            Some(value) => format!("WidgetStatePropertyAll({value:?})"),
            None => "WidgetStatePropertyAll(null)".to_string(),
        },
        None => format!("{property:?}"),
    }
}

/// [`widget_state_property`] with a value description: Dart's `WidgetStatePropertyAll(value)`
/// around `describe(value)`, and Rust's `Some`/`None` kept out of the text.
pub(crate) fn widget_state_property_with<T: Clone + PartialEq + 'static>(
    property: &WidgetStatePropertyRef<Option<T>>,
    describe: impl Fn(&T) -> String,
) -> String {
    let property: &dyn WidgetStateProperty<Option<T>> = &**property;
    match property
        .as_any()
        .downcast_ref::<WidgetStatePropertyAll<Option<T>>>()
    {
        Some(all) => match &all.value {
            Some(value) => format!("WidgetStatePropertyAll({})", describe(value)),
            None => "WidgetStatePropertyAll(null)".to_string(),
        },
        None => format!("{property:?}"),
    }
}

/// Dart's `Set<WidgetState>.toString`, as `{WidgetState.disabled, WidgetState.hovered}`. A
/// Rust set has no insertion order, so the states are sorted to keep a dump stable.
pub(crate) fn widget_states(states: &WidgetStates) -> String {
    let mut names: Vec<String> = states
        .iter()
        .map(|state| typed_enum("WidgetState", state))
        .collect();
    names.sort();
    format!("{{{}}}", names.join(", "))
}

/// [`widget_state_property`] for a `WidgetStateProperty<OutlinedBorder?>`, whose Dart value
/// prints through `ShapeBorder`'s `toString`.
pub(crate) fn widget_state_property_shape(
    property: &WidgetStatePropertyRef<Option<waft_painting::OutlinedBorderRef>>,
) -> String {
    widget_state_property_with(property, |border| {
        waft_painting::ShapeBorderRef::from(border.clone()).to_string()
    })
}
