//! Key code translation.
//!
//! The wire format identifies a key by its physical position, as a USB HID
//! usage on the keyboard page (0x07). Each OS backend converts its native
//! codes to and from HID; the receiving OS's keyboard layout then decides
//! which character a key produces (so both machines should use the same
//! layout, e.g. Turkish Q).

/// HID usages the rest of the code refers to by name.
pub mod usage {
    pub const ESCAPE: u16 = 0x29;
    pub const CAPS_LOCK: u16 = 0x39;
    pub const LEFT_CTRL: u16 = 0xE0;
    pub const LEFT_SHIFT: u16 = 0xE1;
    pub const LEFT_ALT: u16 = 0xE2;
    pub const LEFT_GUI: u16 = 0xE3;
    pub const RIGHT_CTRL: u16 = 0xE4;
    pub const RIGHT_SHIFT: u16 = 0xE5;
    pub const RIGHT_ALT: u16 = 0xE6;
    pub const RIGHT_GUI: u16 = 0xE7;

    pub fn is_modifier(usage: u16) -> bool {
        (LEFT_CTRL..=RIGHT_GUI).contains(&usage)
    }
}

const NO: u16 = 0xFFFF;

/// (HID usage, Windows set-1 scan code, macOS virtual key code).
///
/// Windows extended keys carry `0xE0` in the high byte. Where two HID usages
/// share one native code, the first row wins for native-to-HID lookups.
#[rustfmt::skip]
static KEYS: &[(u16, u16, u16)] = &[
    // Letters
    (0x04, 0x1E, 0x00), // A
    (0x05, 0x30, 0x0B), // B
    (0x06, 0x2E, 0x08), // C
    (0x07, 0x20, 0x02), // D
    (0x08, 0x12, 0x0E), // E
    (0x09, 0x21, 0x03), // F
    (0x0A, 0x22, 0x05), // G
    (0x0B, 0x23, 0x04), // H
    (0x0C, 0x17, 0x22), // I
    (0x0D, 0x24, 0x26), // J
    (0x0E, 0x25, 0x28), // K
    (0x0F, 0x26, 0x25), // L
    (0x10, 0x32, 0x2E), // M
    (0x11, 0x31, 0x2D), // N
    (0x12, 0x18, 0x1F), // O
    (0x13, 0x19, 0x23), // P
    (0x14, 0x10, 0x0C), // Q
    (0x15, 0x13, 0x0F), // R
    (0x16, 0x1F, 0x01), // S
    (0x17, 0x14, 0x11), // T
    (0x18, 0x16, 0x20), // U
    (0x19, 0x2F, 0x09), // V
    (0x1A, 0x11, 0x0D), // W
    (0x1B, 0x2D, 0x07), // X
    (0x1C, 0x15, 0x10), // Y
    (0x1D, 0x2C, 0x06), // Z
    // Digit row
    (0x1E, 0x02, 0x12), // 1
    (0x1F, 0x03, 0x13), // 2
    (0x20, 0x04, 0x14), // 3
    (0x21, 0x05, 0x15), // 4
    (0x22, 0x06, 0x17), // 5
    (0x23, 0x07, 0x16), // 6
    (0x24, 0x08, 0x1A), // 7
    (0x25, 0x09, 0x1C), // 8
    (0x26, 0x0A, 0x19), // 9
    (0x27, 0x0B, 0x1D), // 0
    // Editing and punctuation
    (0x28, 0x1C, 0x24),   // Enter
    (0x29, 0x01, 0x35),   // Escape
    (0x2A, 0x0E, 0x33),   // Backspace
    (0x2B, 0x0F, 0x30),   // Tab
    (0x2C, 0x39, 0x31),   // Space
    (0x2D, 0x0C, 0x1B),   // - (TR-Q: *)
    (0x2E, 0x0D, 0x18),   // = (TR-Q: -)
    (0x2F, 0x1A, 0x21),   // [ (TR-Q: ğ)
    (0x30, 0x1B, 0x1E),   // ] (TR-Q: ü)
    (0x31, 0x2B, 0x2A),   // \ (TR-Q: ,)
    (0x32, 0x2B, 0x2A),   // ISO key next to Enter; same native code as \
    (0x33, 0x27, 0x29),   // ; (TR-Q: ş)
    (0x34, 0x28, 0x27),   // ' (TR-Q: i)
    (0x35, 0x29, 0x32),   // ` (TR-Q: ")
    (0x36, 0x33, 0x2B),   // , (TR-Q: ö)
    (0x37, 0x34, 0x2F),   // . (TR-Q: ç)
    (0x38, 0x35, 0x2C),   // / (TR-Q: .)
    (0x39, 0x3A, 0x39),   // Caps Lock
    // Function keys
    (0x3A, 0x3B, 0x7A),   // F1
    (0x3B, 0x3C, 0x78),   // F2
    (0x3C, 0x3D, 0x63),   // F3
    (0x3D, 0x3E, 0x76),   // F4
    (0x3E, 0x3F, 0x60),   // F5
    (0x3F, 0x40, 0x61),   // F6
    (0x40, 0x41, 0x62),   // F7
    (0x41, 0x42, 0x64),   // F8
    (0x42, 0x43, 0x65),   // F9
    (0x43, 0x44, 0x6D),   // F10
    (0x44, 0x57, 0x67),   // F11
    (0x45, 0x58, 0x6F),   // F12
    // Navigation
    (0x46, 0xE037, NO),   // Print Screen
    (0x47, 0x46, NO),     // Scroll Lock
    (0x48, 0x45, NO),     // Pause (Windows reports it as non-extended 0x45)
    (0x49, 0xE052, 0x72), // Insert (Mac: Help)
    (0x4A, 0xE047, 0x73), // Home
    (0x4B, 0xE049, 0x74), // Page Up
    (0x4C, 0xE053, 0x75), // Delete
    (0x4D, 0xE04F, 0x77), // End
    (0x4E, 0xE051, 0x79), // Page Down
    (0x4F, 0xE04D, 0x7C), // Right
    (0x50, 0xE04B, 0x7B), // Left
    (0x51, 0xE050, 0x7D), // Down
    (0x52, 0xE048, 0x7E), // Up
    // Keypad
    (0x53, 0xE045, 0x47), // Num Lock (Mac: keypad Clear)
    (0x54, 0xE035, 0x4B), // KP /
    (0x55, 0x37, 0x43),   // KP *
    (0x56, 0x4A, 0x4E),   // KP -
    (0x57, 0x4E, 0x45),   // KP +
    (0x58, 0xE01C, 0x4C), // KP Enter
    (0x59, 0x4F, 0x53),   // KP 1
    (0x5A, 0x50, 0x54),   // KP 2
    (0x5B, 0x51, 0x55),   // KP 3
    (0x5C, 0x4B, 0x56),   // KP 4
    (0x5D, 0x4C, 0x57),   // KP 5
    (0x5E, 0x4D, 0x58),   // KP 6
    (0x5F, 0x47, 0x59),   // KP 7
    (0x60, 0x48, 0x5B),   // KP 8
    (0x61, 0x49, 0x5C),   // KP 9
    (0x62, 0x52, 0x52),   // KP 0
    (0x63, 0x53, 0x41),   // KP .
    (0x67, 0x59, 0x51),   // KP =
    // ISO key left of Z (TR-Q: <). On ISO Macs this position may report
    // kVK_ISO_Section and swap with the ` key; verify on real hardware.
    (0x64, 0x56, 0x0A),
    (0x65, 0xE05D, 0x6E), // Application / context menu
    (0x68, 0x64, 0x69),   // F13
    (0x69, 0x65, 0x6B),   // F14
    (0x6A, 0x66, 0x71),   // F15
    (0x6B, 0x67, 0x6A),   // F16
    (0x6C, 0x68, 0x40),   // F17
    (0x6D, 0x69, 0x4F),   // F18
    (0x6E, 0x6A, 0x50),   // F19
    (0x6F, 0x6B, 0x5A),   // F20
    (0x70, 0x6C, NO),     // F21
    (0x71, 0x6D, NO),     // F22
    (0x72, 0x6E, NO),     // F23
    (0x73, 0x76, NO),     // F24
    (0x87, 0x73, 0x5E),   // JIS Ro
    (0x89, 0x7D, 0x5D),   // JIS Yen
    // Modifiers
    (0xE0, 0x1D, 0x3B),   // Left Ctrl
    (0xE1, 0x2A, 0x38),   // Left Shift
    (0xE2, 0x38, 0x3A),   // Left Alt / Option
    (0xE3, 0xE05B, 0x37), // Left Windows / Command
    (0xE4, 0xE01D, 0x3E), // Right Ctrl
    (0xE5, 0x36, 0x3C),   // Right Shift
    (0xE6, 0xE038, 0x3D), // Right Alt (AltGr) / Option
    (0xE7, 0xE05C, 0x36), // Right Windows / Command
];

fn find(native: u16, column: fn(&(u16, u16, u16)) -> u16) -> Option<u16> {
    if native == NO {
        return None;
    }
    KEYS.iter().find(|row| column(row) == native).map(|row| row.0)
}

fn lookup(usage: u16, column: fn(&(u16, u16, u16)) -> u16) -> Option<u16> {
    KEYS.iter().find(|row| row.0 == usage).map(column).filter(|&code| code != NO)
}

pub fn hid_from_win(scan: u16) -> Option<u16> {
    // Some keys (e.g. Right Shift) occasionally arrive with a stray extended flag.
    find(scan, |r| r.1).or_else(|| find(scan & 0xFF, |r| r.1))
}

pub fn win_from_hid(usage: u16) -> Option<u16> {
    lookup(usage, |r| r.1)
}

pub fn hid_from_mac(keycode: u16) -> Option<u16> {
    find(keycode, |r| r.2)
}

pub fn mac_from_hid(usage: u16) -> Option<u16> {
    lookup(usage, |r| r.2)
}

/// Swaps Ctrl with Windows/Command, so that Ctrl+C on a PC keyboard becomes
/// Cmd+C on a Mac and the other way round.
pub fn swap_ctrl_gui(key: u16) -> u16 {
    match key {
        usage::LEFT_CTRL => usage::LEFT_GUI,
        usage::LEFT_GUI => usage::LEFT_CTRL,
        usage::RIGHT_CTRL => usage::RIGHT_GUI,
        usage::RIGHT_GUI => usage::RIGHT_CTRL,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hid_usages_are_unique() {
        for (i, row) in KEYS.iter().enumerate() {
            assert!(KEYS[i + 1..].iter().all(|other| other.0 != row.0), "duplicate {:#x}", row.0);
        }
    }

    #[test]
    fn native_codes_round_trip() {
        for &(hid, win, mac) in KEYS {
            if win != NO {
                let back = hid_from_win(win).unwrap();
                assert_eq!(win_from_hid(back), Some(win), "win {win:#x} via hid {hid:#x}");
            }
            if mac != NO {
                let back = hid_from_mac(mac).unwrap();
                assert_eq!(mac_from_hid(back), Some(mac), "mac {mac:#x} via hid {hid:#x}");
            }
        }
    }

    #[test]
    fn known_keys() {
        assert_eq!(hid_from_win(0x1E), Some(0x04)); // A
        assert_eq!(hid_from_win(0xE04B), Some(0x50)); // Left arrow
        assert_eq!(hid_from_win(0x4B), Some(0x5C)); // KP 4
        assert_eq!(hid_from_win(0xE036), Some(usage::RIGHT_SHIFT)); // stray extended flag
        assert_eq!(mac_from_hid(0x04), Some(0x00)); // A
        assert_eq!(mac_from_hid(usage::LEFT_GUI), Some(0x37)); // Command
        assert_eq!(mac_from_hid(0x46), None); // Print Screen
        assert_eq!(hid_from_mac(0x3F), None); // Fn has no HID usage
    }

    #[test]
    fn swap_is_an_involution() {
        for key in [usage::LEFT_CTRL, usage::LEFT_GUI, usage::RIGHT_CTRL, usage::RIGHT_GUI, 0x04] {
            assert_eq!(swap_ctrl_gui(swap_ctrl_gui(key)), key);
        }
        assert_eq!(swap_ctrl_gui(usage::LEFT_CTRL), usage::LEFT_GUI);
    }
}
