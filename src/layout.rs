use windows::Win32::Foundation::RECT;

pub const VK_BACK: u16 = 0x08;
pub const VK_TAB: u16 = 0x09;
pub const VK_RETURN: u16 = 0x0D;
pub const VK_CAPITAL: u16 = 0x14;
pub const VK_ESCAPE: u16 = 0x1B;
pub const VK_SPACE: u16 = 0x20;
pub const VK_LEFT: u16 = 0x25;
pub const VK_UP: u16 = 0x26;
pub const VK_RIGHT: u16 = 0x27;
pub const VK_DOWN: u16 = 0x28;

/// Width of every row, in key units.
pub const ROW_UNITS: f32 = 15.0;
pub const ROWS: i32 = 5;

#[derive(Clone, Copy, PartialEq)]
pub enum Act {
    Vk,
    Shift,
    Ctrl,
    Alt,
    Caps,
    Layout,
    Hide,
}

#[derive(Clone, Copy)]
pub struct Key {
    pub label: &'static str,
    pub shifted: &'static str,
    pub vk: u16,
    pub w: f32,
    pub act: Act,
}

impl Key {
    pub fn is_letter(&self) -> bool {
        self.act == Act::Vk && (b'A' as u16..=b'Z' as u16).contains(&self.vk)
    }

    /// Keys that keep firing while the mouse button is held down.
    pub fn repeats(&self) -> bool {
        self.act == Act::Vk
            && matches!(
                self.vk,
                VK_BACK | VK_SPACE | VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN
            )
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Layout {
    Qwerty,
    Abc,
}

impl Layout {
    pub fn toggle(self) -> Self {
        match self {
            Layout::Qwerty => Layout::Abc,
            Layout::Abc => Layout::Qwerty,
        }
    }

    pub fn from_u32(v: u32) -> Self {
        if v == 1 { Layout::Abc } else { Layout::Qwerty }
    }

    pub fn to_u32(self) -> u32 {
        match self {
            Layout::Qwerty => 0,
            Layout::Abc => 1,
        }
    }
}

const LOWER: [&str; 26] = [
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s",
    "t", "u", "v", "w", "x", "y", "z",
];
const UPPER: [&str; 26] = [
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S",
    "T", "U", "V", "W", "X", "Y", "Z",
];

fn k(label: &'static str, shifted: &'static str, vk: u16, w: f32) -> Key {
    Key { label, shifted, vk, w, act: Act::Vk }
}

fn m(label: &'static str, vk: u16, w: f32, act: Act) -> Key {
    Key { label, shifted: label, vk, w, act }
}

fn letters(from: usize, to: usize) -> impl Iterator<Item = Key> {
    (from..to).map(|i| k(LOWER[i], UPPER[i], b'A' as u16 + i as u16, 1.0))
}

fn qwerty_letters(s: &'static str) -> impl Iterator<Item = Key> {
    s.bytes().map(|b| {
        let i = (b - b'a') as usize;
        k(LOWER[i], UPPER[i], b.to_ascii_uppercase() as u16, 1.0)
    })
}

pub fn rows(layout: Layout) -> [Vec<Key>; 5] {
    let mut r0 = vec![
        k("`", "~", 0xC0, 1.0),
        k("1", "!", 0x31, 1.0),
        k("2", "@", 0x32, 1.0),
        k("3", "#", 0x33, 1.0),
        k("4", "$", 0x34, 1.0),
        k("5", "%", 0x35, 1.0),
        k("6", "^", 0x36, 1.0),
        k("7", "&", 0x37, 1.0),
        k("8", "*", 0x38, 1.0),
        k("9", "(", 0x39, 1.0),
        k("0", ")", 0x30, 1.0),
        k("-", "_", 0xBD, 1.0),
        k("=", "+", 0xBB, 1.0),
    ];
    r0.push(k("Bksp", "Bksp", VK_BACK, 2.0));

    let mut r1 = vec![k("Tab", "Tab", VK_TAB, 1.5)];
    let mut r2 = vec![m("Caps", VK_CAPITAL, 1.75, Act::Caps)];
    let mut r3 = vec![m("Shift", 0, 2.25, Act::Shift)];
    match layout {
        Layout::Qwerty => {
            r1.extend(qwerty_letters("qwertyuiop"));
            r2.extend(qwerty_letters("asdfghjkl"));
            r3.extend(qwerty_letters("zxcvbnm"));
        }
        Layout::Abc => {
            r1.extend(letters(0, 10));
            r2.extend(letters(10, 19));
            r3.extend(letters(19, 26));
        }
    }
    r1.extend([
        k("[", "{", 0xDB, 1.0),
        k("]", "}", 0xDD, 1.0),
        k("\\", "|", 0xDC, 1.5),
    ]);
    r2.extend([
        k(";", ":", 0xBA, 1.0),
        k("'", "\"", 0xDE, 1.0),
        k("Enter", "Enter", VK_RETURN, 2.25),
    ]);
    r3.extend([
        k(",", "<", 0xBC, 1.0),
        k(".", ">", 0xBE, 1.0),
        k("/", "?", 0xBF, 1.0),
        m("Shift", 0, 2.75, Act::Shift),
    ]);

    let r4 = vec![
        m("Ctrl", 0, 1.5, Act::Ctrl),
        m("Alt", 0, 1.5, Act::Alt),
        k("Space", "Space", VK_SPACE, 4.5),
        k("Esc", "Esc", VK_ESCAPE, 1.0),
        m(
            match layout {
                Layout::Qwerty => "ABC",
                Layout::Abc => "QWERTY",
            },
            0,
            1.5,
            Act::Layout,
        ),
        m("Hide", 0, 1.0, Act::Hide),
        k("<", "<", VK_LEFT, 1.0),
        k("v", "v", VK_DOWN, 1.0),
        k("^", "^", VK_UP, 1.0),
        k(">", ">", VK_RIGHT, 1.0),
    ];
    [r0, r1, r2, r3, r4]
}

pub fn title_h(unit: i32) -> i32 {
    unit * 45 / 100
}

pub fn pad(unit: i32) -> i32 {
    unit / 10
}

pub fn window_size(unit: i32) -> (i32, i32) {
    let p = pad(unit);
    (
        (ROW_UNITS * unit as f32).round() as i32 + 2 * p,
        title_h(unit) + ROWS * unit + 2 * p,
    )
}

/// Every key of the layout with its client-area rectangle.
pub fn placed(layout: Layout, unit: i32) -> Vec<(RECT, Key)> {
    let p = pad(unit);
    let top = title_h(unit) + p;
    let u = unit as f32;
    let mut out = Vec::with_capacity(64);
    for (row, keys) in rows(layout).iter().enumerate() {
        let mut x = 0.0f32;
        for key in keys {
            out.push((
                RECT {
                    left: p + (x * u).round() as i32,
                    top: top + row as i32 * unit,
                    right: p + ((x + key.w) * u).round() as i32,
                    bottom: top + (row as i32 + 1) * unit,
                },
                *key,
            ));
            x += key.w;
        }
    }
    out
}

pub fn hit(layout: Layout, unit: i32, x: i32, y: i32) -> Option<usize> {
    placed(layout, unit)
        .iter()
        .position(|(r, _)| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
}
