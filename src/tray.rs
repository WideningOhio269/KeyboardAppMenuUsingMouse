use std::ffi::c_void;

use windows::{
    Win32::{
        Foundation::*,
        System::{
            LibraryLoader::{GetModuleFileNameW, GetModuleHandleW},
            Registry::*,
            Threading::CreateMutexW,
        },
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{PCWSTR, w},
};

pub const WM_TRAY: u32 = WM_APP + 1;

pub const ID_TOGGLE: usize = 1;
pub const ID_LAYOUT: usize = 2;
pub const ID_HOT_CORNER: usize = 3;
pub const ID_AUTOSTART: usize = 4;
pub const ID_SIZE_BASE: usize = 10; // 10, 11, 12 = small, medium, large
pub const ID_QUIT: usize = 9;
pub const ID_POPUPS: usize = 5;

const APP_KEY: PCWSTR = w!("Software\\KeyboardAppMenuUsingMouse");
const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_NAME: PCWSTR = w!("KeyboardAppMenuUsingMouse");

/// The icon embedded in the exe (resource id 1, see build.rs).
pub fn app_icon(hinstance: HINSTANCE) -> HICON {
    unsafe {
        LoadIconW(Some(hinstance), PCWSTR(1 as *const u16))
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .unwrap_or_default()
    }
}

pub fn reg_get(name: PCWSTR) -> Option<u32> {
    let mut value = 0u32;
    let mut size = 4u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            APP_KEY,
            name,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    (r == ERROR_SUCCESS).then_some(value)
}

pub fn reg_set(name: PCWSTR, value: u32) {
    unsafe {
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            APP_KEY,
            name,
            REG_DWORD.0,
            Some(&value as *const u32 as *const c_void),
            4,
        );
    }
}

pub fn autostart_enabled() -> bool {
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_NAME, RRF_RT_REG_SZ, None, None, None)
            == ERROR_SUCCESS
    }
}

pub fn set_autostart(on: bool) {
    unsafe {
        if !on {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_NAME);
            return;
        }
        let mut path = [0u16; 520];
        let len = GetModuleFileNameW(None, &mut path) as usize;
        let mut cmd: Vec<u16> = "\"".encode_utf16().collect();
        cmd.extend_from_slice(&path[..len]);
        cmd.extend("\" --hidden".encode_utf16());
        cmd.push(0);
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            RUN_NAME,
            REG_SZ.0,
            Some(cmd.as_ptr() as *const c_void),
            (cmd.len() * 2) as u32,
        );
    }
}

/// Returns false when another copy is already running.
pub fn single_instance() -> bool {
    unsafe {
        let handle = CreateMutexW(None, true, w!("Local\\KeyboardAppMenuUsingMouse"));
        // The handle is intentionally never closed; it lives as long as the process.
        handle.is_ok() && GetLastError() != ERROR_ALREADY_EXISTS
    }
}

fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        ..Default::default()
    };
    let hmodule = unsafe { GetModuleHandleW(None).unwrap_or_default() };
    nid.hIcon = app_icon(HINSTANCE(hmodule.0));
    for (dst, src) in nid.szTip.iter_mut().zip("Mouse Keyboard".encode_utf16()) {
        *dst = src;
    }
    nid
}

pub fn add_icon(hwnd: HWND) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &icon_data(hwnd));
    }
}

pub fn remove_icon(hwnd: HWND) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &icon_data(hwnd));
    }
}

pub struct MenuState {
    pub visible: bool,
    pub other_layout: &'static str,
    pub size: usize,
    pub hot_corner: bool,
    pub popups: bool,
    pub autostart: bool,
}

/// Shows the tray menu at the cursor and returns the chosen command id (0 if dismissed).
pub fn show_menu(hwnd: HWND, st: &MenuState) -> usize {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return 0 };
        let check = |on: bool| if on { MF_CHECKED } else { MF_UNCHECKED };

        let _ = AppendMenuW(
            menu,
            MF_STRING,
            ID_TOGGLE,
            if st.visible { w!("Hide keyboard") } else { w!("Show keyboard") },
        );
        let layout_text: Vec<u16> = format!("Switch to {} layout\0", st.other_layout)
            .encode_utf16()
            .collect();
        let _ = AppendMenuW(menu, MF_STRING, ID_LAYOUT, PCWSTR(layout_text.as_ptr()));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let sizes = [w!("Size: small"), w!("Size: medium"), w!("Size: large")];
        for (i, text) in sizes.into_iter().enumerate() {
            let _ = AppendMenuW(menu, MF_STRING | check(st.size == i), ID_SIZE_BASE + i, text);
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(
            menu,
            MF_STRING | check(st.hot_corner),
            ID_HOT_CORNER,
            w!("Show on top-left screen corner"),
        );
        let _ = AppendMenuW(
            menu,
            MF_STRING | check(st.popups),
            ID_POPUPS,
            w!("Show key pop-ups"),
        );
        let _ = AppendMenuW(
            menu,
            MF_STRING | check(st.autostart),
            ID_AUTOSTART,
            w!("Start with Windows"),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, w!("Quit"));

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Required so the menu closes when the user clicks elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            pt.x,
            pt.y,
            None,
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        cmd.0 as usize
    }
}
