//! USB HID keyboard usages (page 7, what the device sends) to PC set-1 scan codes, which
//! `SendInput` with `KEYEVENTF_SCANCODE` takes, plus the extended (E0) flag.

/// A key as Windows takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Scan {
    pub code: u16,
    pub extended: bool,
}

const fn k(code: u16) -> Option<Scan> {
    Some(Scan { code, extended: false })
}
const fn e(code: u16) -> Option<Scan> {
    Some(Scan { code, extended: true })
}

pub const LEFT_CTRL: Scan = Scan { code: 0x1D, extended: false };
pub const RIGHT_CTRL: Scan = Scan { code: 0x1D, extended: true };
pub const LEFT_SHIFT: Scan = Scan { code: 0x2A, extended: false };
pub const RIGHT_SHIFT: Scan = Scan { code: 0x36, extended: false };
pub const LEFT_ALT: Scan = Scan { code: 0x38, extended: false };
pub const RIGHT_ALT: Scan = Scan { code: 0x38, extended: true };
pub const LEFT_WIN: Scan = Scan { code: 0x5B, extended: true };
pub const ENTER: Scan = Scan { code: 0x1C, extended: false };
pub const TAB: Scan = Scan { code: 0x0F, extended: false };
pub const BACKSPACE: Scan = Scan { code: 0x0E, extended: false };
pub const SPACE: Scan = Scan { code: 0x39, extended: false };

/// HID usages of the modifier keys.
pub const HID_LEFT_CTRL: u16 = 0xE0;
pub const HID_LEFT_SHIFT: u16 = 0xE1;
pub const HID_LEFT_ALT: u16 = 0xE2;
pub const HID_LEFT_GUI: u16 = 0xE3;
pub const HID_RIGHT_CTRL: u16 = 0xE4;
pub const HID_RIGHT_SHIFT: u16 = 0xE5;
pub const HID_RIGHT_ALT: u16 = 0xE6;
pub const HID_RIGHT_GUI: u16 = 0xE7;
pub const HID_SPACE: u16 = 0x2C;

/// The scan code for a HID usage, `None` for usages with no PC key (or none worth sending, such
/// as Pause's three-byte sequence and the system keys).
pub fn scan(usage: u16) -> Option<Scan> {
    // Letters a..z in HID order.
    const LETTERS: [u16; 26] = [
        0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18, 0x19, 0x10, 0x13,
        0x1F, 0x14, 0x16, 0x2F, 0x11, 0x2D, 0x15, 0x2C,
    ];
    match usage {
        0x04..=0x1D => k(LETTERS[(usage - 0x04) as usize]),
        // 1..9 then 0.
        0x1E..=0x27 => k(0x02 + (usage - 0x1E)),
        0x28 => k(0x1C), // Return
        0x29 => k(0x01), // Escape
        0x2A => k(0x0E), // Backspace
        0x2B => k(0x0F), // Tab
        0x2C => k(0x39), // Space
        0x2D => k(0x0C), // - _
        0x2E => k(0x0D), // = +
        0x2F => k(0x1A), // [ {
        0x30 => k(0x1B), // ] }
        0x31 => k(0x2B), // \ |
        0x32 => k(0x2B), // non-US # ~
        0x33 => k(0x27), // ; :
        0x34 => k(0x28), // ' "
        0x35 => k(0x29), // ` ~
        0x36 => k(0x33), // , <
        0x37 => k(0x34), // . >
        0x38 => k(0x35), // / ?
        0x39 => k(0x3A), // Caps Lock
        0x3A..=0x43 => k(0x3B + (usage - 0x3A)), // F1..F10
        0x44 => k(0x57),                         // F11
        0x45 => k(0x58),                         // F12
        0x46 => e(0x37),                         // Print Screen
        0x47 => k(0x46),                         // Scroll Lock
        0x49 => e(0x52),                         // Insert
        0x4A => e(0x47),                         // Home
        0x4B => e(0x49),                         // Page Up
        0x4C => e(0x53),                         // Delete (forward)
        0x4D => e(0x4F),                         // End
        0x4E => e(0x51),                         // Page Down
        0x4F => e(0x4D),                         // Right
        0x50 => e(0x4B),                         // Left
        0x51 => e(0x50),                         // Down
        0x52 => e(0x48),                         // Up
        0x53 => e(0x45),                         // Num Lock
        0x54 => e(0x35),                         // keypad /
        0x55 => k(0x37),                         // keypad *
        0x56 => k(0x4A),                         // keypad -
        0x57 => k(0x4E),                         // keypad +
        0x58 => e(0x1C),                         // keypad Enter
        0x59 => k(0x4F),                         // keypad 1
        0x5A => k(0x50),
        0x5B => k(0x51),
        0x5C => k(0x4B),
        0x5D => k(0x4C),
        0x5E => k(0x4D),
        0x5F => k(0x47),
        0x60 => k(0x48),
        0x61 => k(0x49), // keypad 9
        0x62 => k(0x52), // keypad 0
        0x63 => k(0x53), // keypad .
        0x64 => k(0x56), // non-US \ |
        0x65 => e(0x5D), // Application (menu)
        0x67 => k(0x59), // keypad =
        0x68..=0x6E => k(0x64 + (usage - 0x68)), // F13..F19
        0x6F..=0x72 => k(0x6B + (usage - 0x6F)), // F20..F23
        0x73 => k(0x76),                         // F24
        HID_LEFT_CTRL => Some(LEFT_CTRL),
        HID_LEFT_SHIFT => Some(LEFT_SHIFT),
        HID_LEFT_ALT => Some(LEFT_ALT),
        HID_LEFT_GUI => Some(LEFT_WIN),
        HID_RIGHT_CTRL => Some(RIGHT_CTRL),
        HID_RIGHT_SHIFT => Some(RIGHT_SHIFT),
        HID_RIGHT_ALT => Some(RIGHT_ALT),
        HID_RIGHT_GUI => e(0x5C),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_digits_and_navigation() {
        assert_eq!(scan(0x04), k(0x1E)); // a
        assert_eq!(scan(0x1D), k(0x2C)); // z
        assert_eq!(scan(0x06), k(0x2E)); // c
        assert_eq!(scan(0x19), k(0x2F)); // v
        assert_eq!(scan(0x1E), k(0x02)); // 1
        assert_eq!(scan(0x27), k(0x0B)); // 0
        assert_eq!(scan(0x3A), k(0x3B)); // F1
        assert_eq!(scan(0x43), k(0x44)); // F10
        assert_eq!(scan(0x45), k(0x58)); // F12
        assert_eq!(scan(0x50), e(0x4B)); // Left arrow is extended
        assert_eq!(scan(0x4C), e(0x53)); // forward Delete is extended
        assert_eq!(scan(0x58), e(0x1C)); // keypad Enter is extended
        assert_eq!(scan(0x28), k(0x1C)); // Return is not
        assert_eq!(scan(0x6F), k(0x6B)); // F20
    }

    #[test]
    fn unknown_usages_send_nothing() {
        for u in [0x00, 0x01, 0x03, 0x48, 0x66, 0x74, 0xA5, 0xDF, 0xE8, 0xFFFF] {
            assert_eq!(scan(u), None, "usage {u:#x}");
        }
    }
}
