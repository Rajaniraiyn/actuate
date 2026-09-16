//! XKB keymap text generation for virtual keyboards.
//!
//! Wayland virtual keyboards deliver key codes, never characters. Typing
//! arbitrary Unicode therefore needs a keymap whose key codes map to exactly
//! the requested code points. The seat's real keymap is used for physical
//! key chords so evdev codes keep the user's layout semantics.
use std::collections::BTreeMap;

/// evdev key codes carry an XKB offset of 8 in keymaps and protocol events.
pub const XKB_OFFSET: u32 = 8;
/// Modifier bit positions in the conventional pc/evdev keymaps.
pub const MOD_SHIFT: u32 = 1 << 0;
pub const MOD_CAPS: u32 = 1 << 1;
pub const MOD_CONTROL: u32 = 1 << 2;
pub const MOD_ALT: u32 = 1 << 3;
pub const MOD_NUM: u32 = 1 << 4;
pub const MOD_SUPER: u32 = 1 << 6;
/// Largest number of distinct characters one generated keymap can hold.
pub const MAX_SYMBOLS: usize = 240;
/// Typeable control characters with their XKB name and X keysym.
pub const CONTROL_KEYSYMS: [(char, &str, u32); 4] = [
    ('\n', "Return", 0xff0d),
    ('\t', "Tab", 0xff09),
    ('\u{8}', "BackSpace", 0xff08),
    ('\u{1b}', "Escape", 0xff1b),
];

fn keysym_name(c: char) -> String {
    if let Some((_, name, _)) = CONTROL_KEYSYMS.iter().find(|(ch, _, _)| *ch == c) {
        return (*name).into();
    }
    if c == ' ' {
        return "space".into();
    }
    format!("U{:04X}", c as u32)
}

/// A keymap that maps one key code per distinct character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnicodeKeymap {
    pub text: String,
    codes: BTreeMap<char, u32>,
}
impl UnicodeKeymap {
    /// Builds a keymap covering every character in `text`, in first-seen
    /// order. Returns `None` when more than `MAX_SYMBOLS` distinct
    /// characters are needed; callers split the text and swap keymaps.
    pub fn for_text(text: &str) -> Option<Self> {
        let mut codes = BTreeMap::new();
        let mut order = Vec::new();
        for c in text.chars() {
            if c == '\r' || (c.is_control() && !CONTROL_KEYSYMS.iter().any(|(ch, _, _)| *ch == c)) {
                continue;
            }
            if let std::collections::btree_map::Entry::Vacant(slot) = codes.entry(c) {
                if order.len() >= MAX_SYMBOLS {
                    return None;
                }
                // Key code 1 is reserved; start at evdev code 1 + 1.
                slot.insert(order.len() as u32 + 2);
                order.push(c);
            }
        }
        let mut keycodes = String::new();
        let mut symbols = String::new();
        for c in &order {
            let code = codes[c];
            keycodes.push_str(&format!("    <K{code}> = {};\n", code + XKB_OFFSET));
            symbols.push_str(&format!(
                "    key <K{code}> {{ [ {} ] }};\n",
                keysym_name(*c)
            ));
        }
        let max = order.len() as u32 + 1 + XKB_OFFSET;
        let text = format!(
            "xkb_keymap {{\n  xkb_keycodes \"unimation\" {{\n    minimum = 8;\n    maximum = {max};\n{keycodes}  }};\n  xkb_types \"unimation\" {{\n    type \"ONE_LEVEL\" {{ modifiers = none; map[none] = Level1; level_name[Level1] = \"Any\"; }};\n  }};\n  xkb_compatibility \"unimation\" {{ }};\n  xkb_symbols \"unimation\" {{\n{symbols}  }};\n}};\n",
        );
        Some(Self { text, codes })
    }
    /// The evdev key code for a character in this keymap.
    pub fn code(&self, c: char) -> Option<u32> {
        self.codes.get(&c).copied()
    }
}

/// Splits text into runs that each fit one generated keymap.
pub fn chunks(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut distinct = std::collections::HashSet::new();
    for c in text.chars() {
        if !distinct.contains(&c) && distinct.len() >= MAX_SYMBOLS {
            chunks.push(std::mem::take(&mut current));
            distinct.clear();
        }
        distinct.insert(c);
        current.push(c);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Modifier mask from the portable modifier flags.
pub fn modifier_mask(modifiers: unimation::Modifiers) -> u32 {
    let mut mask = 0;
    if modifiers.shift {
        mask |= MOD_SHIFT;
    }
    if modifiers.control {
        mask |= MOD_CONTROL;
    }
    if modifiers.alt {
        mask |= MOD_ALT;
    }
    if modifiers.meta {
        mask |= MOD_SUPER;
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keymap_assigns_one_code_per_character_and_names_keysyms() {
        let map = UnicodeKeymap::for_text("Hi🦀\n").unwrap();
        assert_eq!(map.code('H'), Some(2));
        assert_eq!(map.code('🦀'), Some(4));
        assert!(map.text.contains("key <K4> { [ U1F980 ] };"));
        assert!(map.text.contains("key <K5> { [ Return ] };"));
        assert!(map.text.contains("<K2> = 10;"));
        assert!(map.text.contains("maximum = 13;"));
        assert_eq!(map.code('\r'), None);
    }
    #[test]
    fn oversized_text_is_chunked_by_distinct_characters() {
        let text: String = (0..500)
            .map(|i| char::from_u32(0x4E00 + i).unwrap())
            .collect();
        assert!(UnicodeKeymap::for_text(&text).is_none());
        let parts = chunks(&text);
        assert_eq!(parts.len(), 3);
        assert!(parts.iter().all(|p| UnicodeKeymap::for_text(p).is_some()));
        assert_eq!(parts.concat(), text);
    }
    #[test]
    fn modifier_masks_follow_pc_keymap_bits() {
        let mods = unimation::Modifiers {
            shift: true,
            control: true,
            alt: false,
            meta: true,
        };
        assert_eq!(modifier_mask(mods), MOD_SHIFT | MOD_CONTROL | MOD_SUPER);
    }
}
