//! Reading what a widget, a state or a value says about itself, as Flutter's tests read a
//! `DiagnosticPropertiesBuilder`.

use waft_foundation::{DiagnosticLevel, DiagnosticPropertiesBuilder};

/// The properties `fill` adds that show at the info level, each as Dart's
/// `DiagnosticsNode.toString()` prints it: what Flutter's tests collect with
/// `builder.properties.where((node) => !node.isFiltered(DiagnosticLevel.info)).map((node) =>
/// node.toString())`.
///
/// ```ignore
/// let description =
///     describe_properties(|properties| widget.debug_fill_properties(&app, properties));
/// assert_eq!(description, ["width: 10.0"]);
/// ```
pub fn describe_properties(fill: impl FnOnce(&mut DiagnosticPropertiesBuilder)) -> Vec<String> {
    let mut properties = DiagnosticPropertiesBuilder::new();
    fill(&mut properties);
    properties
        .properties
        .iter()
        .filter(|node| !node.is_filtered(DiagnosticLevel::Info))
        .map(|node| node.to_string_with(None, DiagnosticLevel::Info))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RenderObjectWidget, SizedBox};

    /// A property left at its default is filtered out, as in Flutter's description tests.
    #[test]
    fn only_the_properties_shown_at_the_info_level_are_described() {
        let sized = SizedBox::new().width(10.0);
        let cell = waft_foundation::AppCell::new();
        let app = cell.borrow();
        assert_eq!(
            describe_properties(|properties| sized.debug_fill_properties(&app, properties)),
            ["width: 10.0"]
        );
    }
}
