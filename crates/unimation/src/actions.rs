use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    #[default]
    Left,
    Right,
    Middle,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyChord {
    /// Native virtual key code, not a Unicode scalar or assumed keyboard layout.
    pub key_code: u16,
    #[serde(default)]
    pub modifiers: Modifiers,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClickMode {
    Semantic,
    Global,
    Process,
    Skylight,
}
pub const fn one_click() -> u8 {
    1
}
pub const fn default_drag_duration() -> u64 {
    250
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttributeParameter {
    String { value: String },
    Integer { value: i64 },
    Range { location: i64, length: i64 },
    Point { value: crate::Point },
}
