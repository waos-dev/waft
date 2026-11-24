//! XAML `HorizontalAlignment`, for the templates and properties that name one.

/// XAML `HorizontalAlignment`: where an element, or a control's content, sits across the slot it is given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HorizontalAlignment {
    Left,
    Center,
    Right,
    /// The whole width of the slot.
    #[default]
    Stretch,
}
