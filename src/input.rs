use std::mem::size_of;

use windows::Win32::UI::Input::KeyboardAndMouse::*;

#[derive(Clone, Copy, Default)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

fn key_event(vk: u16, up: bool) -> INPUT {
    let mut flags = if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) };
    // Arrow keys must be flagged extended, or apps see them as numpad keys.
    if matches!(vk, 0x25..=0x28 | 0x2E) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Press and release `vk` with the given modifiers held around it.
pub fn tap(vk: u16, mods: Mods) {
    let held: Vec<u16> = [
        (mods.ctrl, VK_CONTROL.0),
        (mods.alt, VK_MENU.0),
        (mods.shift, VK_SHIFT.0),
    ]
    .into_iter()
    .filter_map(|(on, vk)| on.then_some(vk))
    .collect();

    let mut events = Vec::with_capacity(held.len() * 2 + 2);
    events.extend(held.iter().map(|&m| key_event(m, false)));
    events.push(key_event(vk, false));
    events.push(key_event(vk, true));
    events.extend(held.iter().rev().map(|&m| key_event(m, true)));

    unsafe {
        SendInput(&events, size_of::<INPUT>() as i32);
    }
}
