#![windows_subsystem = "windows"]

mod input;
mod layout;
mod tray;

use std::cell::Cell;

use input::{Mods, tap};
use layout::{Act, Layout, VK_CAPITAL, hit, placed, title_h, window_size};
use tray::*;
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
    core::w,
};

const WM_SHOW_KEYBOARD: u32 = WM_APP + 2;
const TIMER_REPEAT: usize = 1;
const TIMER_LONG: usize = 2;
const TIMER_FLAG: usize = 3;
const TIMER_FADE: usize = 4;
const FADE_TICK_MS: u32 = 16;
/// Highlight lost per tick; 255 / 10 per 16 ms ≈ 0.4 s fade.
const FADE_STEP: u8 = 10;
const MAX_KEYS: usize = 64;
const LONG_PRESS_MS: u32 = 450;
const REPEAT_DELAY_MS: u32 = 500;
const REPEAT_RATE_MS: u32 = 60;
/// Key size in pixels at 96 dpi for the small / medium / large settings.
const UNIT_BASE: [i32; 3] = [48, 64, 80];

#[derive(Clone, Copy)]
struct State {
    hwnd: HWND,
    layout: Layout,
    size: usize,
    unit: i32,
    font_big: HFONT,
    font_small: HFONT,
    font_flag: HFONT,
    /// Two flag windows used alternately, so a flag never changes its text while visible.
    flag_pool: [HWND; 2],
    flag_texts: [&'static str; 2],
    /// Brightness (0-255) of the blue press highlight left on each key, fading to 0.
    fades: [u8; MAX_KEYS],
    flag: HWND,
    flag_size: (i32, i32),
    flag_on: bool,
    /// What the key turns into if the current press becomes a long press.
    long_text: &'static str,
    mods: Mods,
    caps: bool,
    pressed: Option<usize>,
    repeat: Option<(u16, Mods)>,
    /// Key held down that is waiting to see if it becomes a long press.
    long: Option<(u16, Mods)>,
    hot_corner: bool,
    popups: bool,
    hook: HHOOK,
    taskbar_created: u32,
}

thread_local! {
    static ST: Cell<State> = Cell::new(State {
        hwnd: HWND::default(),
        layout: Layout::Qwerty,
        size: 1,
        unit: 64,
        font_big: HFONT::default(),
        font_small: HFONT::default(),
        font_flag: HFONT::default(),
        flag_pool: [HWND(std::ptr::null_mut()); 2],
        flag_texts: [""; 2],
        fades: [0; MAX_KEYS],
        flag: HWND::default(),
        flag_size: (0, 0),
        flag_on: false,
        long_text: "",
        mods: Mods { shift: false, ctrl: false, alt: false },
        caps: false,
        pressed: None,
        repeat: None,
        long: None,
        hot_corner: false,
        popups: false,
        hook: HHOOK::default(),
        taskbar_created: 0,
    });
}

fn rgb(r: u32, g: u32, b: u32) -> COLORREF {
    COLORREF(r | (g << 8) | (b << 16))
}

fn main() {
    if !single_instance() {
        return;
    }
    let start_hidden = std::env::args().any(|a| a == "--hidden");

    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE);

        let hmodule = GetModuleHandleW(None).unwrap_or_default();
        let hinstance = HINSTANCE(hmodule.0);
        let class = w!("KeyboardAppMenuUsingMouseWnd");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hIcon: app_icon(hinstance),
            hIconSm: app_icon(hinstance),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let mut s = ST.get();
        s.layout = Layout::from_u32(reg_get(w!("Layout")).unwrap_or(0));
        s.size = (reg_get(w!("Size")).unwrap_or(1) as usize).min(2);
        s.hot_corner = reg_get(w!("HotCorner")).unwrap_or(0) == 1;
        s.popups = reg_get(w!("Popups")).unwrap_or(0) == 1;
        s.caps = GetKeyState(VK_CAPITAL as i32) & 1 != 0;
        s.unit = UNIT_BASE[s.size] * GetDpiForSystem() as i32 / 96;
        s.taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
        let (ww, wh) = window_size(s.unit);
        let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        let mut x = reg_get(w!("X")).map(|v| v as i32);
        let mut y = reg_get(w!("Y")).map(|v| v as i32);
        if !matches!((x, y), (Some(x), Some(y)) if x >= 0 && y >= 0 && x + ww / 2 < sw && y + wh / 2 < sh) {
            x = Some((sw - ww) / 2);
            y = Some(sh - wh - 80);
        }

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_APPWINDOW,
            class,
            w!("Mouse Keyboard"),
            WS_POPUP | WS_MINIMIZEBOX,
            x.unwrap_or(0),
            y.unwrap_or(0),
            ww,
            wh,
            None,
            None,
            Some(hinstance),
            None,
        )
        .expect("failed to create window");
        s.hwnd = hwnd;
        let (big, small, flag) = make_fonts(s.unit);
        s.font_big = big;
        s.font_small = small;
        s.font_flag = flag;
        let flag_class = w!("KeyboardAppMenuUsingMouseFlag");
        RegisterClassExW(&WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(flag_proc),
            hInstance: hinstance,
            lpszClassName: flag_class,
            ..Default::default()
        });
        for slot in 0..2 {
        s.flag_pool[slot] = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT,
            flag_class,
            w!(""),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(hinstance),
            None,
        )
        .unwrap_or_default();
        }
        s.flag = s.flag_pool[0];
        ST.set(s);

        add_icon(hwnd);
        if s.hot_corner {
            set_hot_corner(true);
        }
        if start_hidden {
            let _ = ShowWindow(hwnd, SW_SHOWMINNOACTIVE);
        } else {
            show_keyboard(hwnd);
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe fn make_fonts(unit: i32) -> (HFONT, HFONT, HFONT) {
    unsafe {
        let make = |height: i32| {
            CreateFontW(
                -height,
                0,
                0,
                0,
                600,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                0,
                w!("Segoe UI"),
            )
        };
        (make(unit * 40 / 100), make(unit * 26 / 100), make(unit * 75 / 100))
    }
}

/// Visible on screen, i.e. not minimized to its taskbar button.
unsafe fn is_shown(hwnd: HWND) -> bool {
    unsafe { IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() }
}

unsafe fn show_keyboard(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

unsafe fn set_hot_corner(on: bool) {
    unsafe {
        let mut s = ST.get();
        if on && s.hook.is_invalid() {
            let hmodule = GetModuleHandleW(None).unwrap_or_default();
            s.hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_ll), Some(HINSTANCE(hmodule.0)), 0)
                .unwrap_or_default();
        } else if !on && !s.hook.is_invalid() {
            let _ = UnhookWindowsHookEx(s.hook);
            s.hook = HHOOK::default();
        }
        ST.set(s);
    }
}

unsafe extern "system" fn mouse_ll(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if code >= 0 && wparam.0 as u32 == WM_MOUSEMOVE {
            let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            if info.pt.x == 0 && info.pt.y == 0 {
                let s = ST.get();
                if !is_shown(s.hwnd) {
                    let _ = PostMessageW(Some(s.hwnd), WM_SHOW_KEYBOARD, WPARAM(0), LPARAM(0));
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }
}

unsafe fn apply_size(hwnd: HWND, size: usize) {
    unsafe {
        let mut s = ST.get();
        s.size = size;
        s.unit = UNIT_BASE[size] * GetDpiForSystem() as i32 / 96;
        let _ = DeleteObject(s.font_big.into());
        let _ = DeleteObject(s.font_small.into());
        let _ = DeleteObject(s.font_flag.into());
        let (big, small, flag) = make_fonts(s.unit);
        s.font_big = big;
        s.font_small = small;
        s.font_flag = flag;
        ST.set(s);
        let (ww, wh) = window_size(s.unit);
        let _ = SetWindowPos(hwnd, None, 0, 0, ww, wh, SWP_NOMOVE | SWP_NOACTIVATE | SWP_NOZORDER);
        let _ = InvalidateRect(Some(hwnd), None, false);
        reg_set(w!("Size"), size as u32);
    }
}

unsafe fn switch_layout(hwnd: HWND) {
    unsafe {
        let mut s = ST.get();
        s.layout = s.layout.toggle();
        s.fades = [0; MAX_KEYS];
        ST.set(s);
        reg_set(w!("Layout"), s.layout.to_u32());
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

unsafe fn on_tray_menu(hwnd: HWND) {
    unsafe {
        let s = ST.get();
        let state = MenuState {
            visible: is_shown(hwnd),
            other_layout: if s.layout == Layout::Qwerty { "ABC" } else { "QWERTY" },
            size: s.size,
            hot_corner: s.hot_corner,
            popups: s.popups,
            autostart: autostart_enabled(),
        };
        match show_menu(hwnd, &state) {
            ID_TOGGLE => {
                if state.visible {
                    let _ = ShowWindow(hwnd, SW_SHOWMINNOACTIVE);
                } else {
                    show_keyboard(hwnd);
                }
            }
            ID_LAYOUT => switch_layout(hwnd),
            ID_HOT_CORNER => {
                let mut s = ST.get();
                s.hot_corner = !s.hot_corner;
                ST.set(s);
                reg_set(w!("HotCorner"), s.hot_corner as u32);
                set_hot_corner(s.hot_corner);
            }
            ID_POPUPS => {
                let mut s = ST.get();
                s.popups = !s.popups;
                ST.set(s);
                reg_set(w!("Popups"), s.popups as u32);
            }
            ID_AUTOSTART => set_autostart(!state.autostart),
            id @ 10..=12 => apply_size(hwnd, id - ID_SIZE_BASE),
            ID_QUIT => {
                let _ = DestroyWindow(hwnd);
            }
            _ => {}
        }
    }
}

unsafe fn on_button_down(hwnd: HWND, x: i32, y: i32) {
    unsafe {
        let mut s = ST.get();
        let keys = placed(s.layout, s.unit);
        let Some(i) = hit(s.layout, s.unit, x, y) else { return };
        let key = keys[i].1;
        let key_rect = keys[i].0;
        let shifted_now = if key.is_letter() { s.mods.shift != s.caps } else { s.mods.shift };
        let (now_text, other_text) = if shifted_now { (key.shifted, key.label) } else { (key.label, key.shifted) };
        match key.act {
            Act::Vk if key.shifted != key.label => {
                // Sent on release (tap) or after a long press (shifted variant).
                s.long = Some((key.vk, s.mods));
                s.mods = Mods::default();
                SetTimer(Some(hwnd), TIMER_LONG, LONG_PRESS_MS, None);
            }
            Act::Vk => {
                tap(key.vk, s.mods);
                if key.repeats() {
                    s.repeat = Some((key.vk, s.mods));
                    SetTimer(Some(hwnd), TIMER_REPEAT, REPEAT_DELAY_MS, None);
                }
                s.mods = Mods::default();
            }
            Act::Shift => s.mods.shift = !s.mods.shift,
            Act::Ctrl => s.mods.ctrl = !s.mods.ctrl,
            Act::Alt => s.mods.alt = !s.mods.alt,
            Act::Caps => {
                tap(VK_CAPITAL, Mods::default());
                s.caps = !s.caps;
            }
            Act::Layout => {
                ST.set(s);
                switch_layout(hwnd);
                s = ST.get();
            }
            Act::Hide => {
                let _ = ShowWindow(hwnd, SW_SHOWMINNOACTIVE);
            }
        }
        s.pressed = Some(i);
        if let Some(f) = s.fades.get_mut(i) {
            *f = 255;
        }
        if s.popups && key.act == Act::Vk && now_text.chars().count() == 1 {
            s.long_text = other_text;
            ST.set(s);
            show_flag(hwnd, key_rect, now_text);
            s = ST.get();
        }
        ST.set(s);
        SetCapture(hwnd);
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

unsafe fn on_button_up(hwnd: HWND) {
    unsafe {
        let mut s = ST.get();
        if let Some((vk, mods)) = s.long.take() {
            tap(vk, mods);
        }
        s.pressed = None;
        s.repeat = None;
        ST.set(s);
        let _ = KillTimer(Some(hwnd), TIMER_REPEAT);
        let _ = KillTimer(Some(hwnd), TIMER_LONG);
        SetTimer(Some(hwnd), TIMER_FADE, FADE_TICK_MS, None);
        if ST.get().flag_on {
            SetTimer(Some(hwnd), TIMER_FLAG, 40, None);
        }
        let _ = ReleaseCapture();
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

/// Renders the flag into an offscreen bitmap and hands it to the window in one atomic
/// step (UpdateLayeredWindow), so the window can never show stale or half-drawn content.
unsafe fn render_flag(flag: HWND, text: &str, w: i32, h: i32, pos: Option<POINT>) {
    unsafe {
        let s = ST.get();
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old_bmp = SelectObject(mem, bmp.into());

        let key = rgb(255, 0, 255);
        let b_key = CreateSolidBrush(key);
        let b_face = CreateSolidBrush(rgb(235, 238, 245));
        let rc = RECT { left: 0, top: 0, right: w, bottom: h };
        FillRect(mem, &rc, b_key);
        let old_brush = SelectObject(mem, b_face.into());
        let old_pen = SelectObject(mem, GetStockObject(NULL_PEN));
        let r = s.unit / 3;
        let _ = RoundRect(mem, 0, 0, w + 1, h + 1, r, r);

        SetBkMode(mem, TRANSPARENT);
        SetTextColor(mem, rgb(24, 26, 31));
        let old_font = SelectObject(mem, s.font_flag.into());
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let mut tr = rc;
        DrawTextW(mem, &mut wide, &mut tr, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

        let size = SIZE { cx: w, cy: h };
        let src = POINT { x: 0, y: 0 };
        let _ = UpdateLayeredWindow(
            flag,
            None,
            pos.as_ref().map(|p| p as *const POINT),
            Some(&size),
            Some(mem),
            Some(&src),
            key,
            None,
            ULW_COLORKEY,
        );

        SelectObject(mem, old_font);
        SelectObject(mem, old_pen);
        SelectObject(mem, old_brush);
        SelectObject(mem, old_bmp);
        let _ = DeleteObject(b_key.into());
        let _ = DeleteObject(b_face.into());
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
    }
}

unsafe fn show_flag(main: HWND, r: RECT, text: &'static str) {
    unsafe {
        let mut s = ST.get();
        // Drop the previous flag instantly and use the other window for the new one.
        let _ = ShowWindow(s.flag, SW_HIDE);
        let next = if s.flag == s.flag_pool[0] { 1 } else { 0 };
        s.flag = s.flag_pool[next];
        s.flag_texts[next] = text;
        s.flag_on = true;
        let _ = KillTimer(Some(main), TIMER_FLAG);
        let u = s.unit;
        let w = (r.right - r.left) + u * 4 / 10;
        let h = u * 13 / 10;
        s.flag_size = (w, h);
        ST.set(s);
        let mut pt = POINT { x: r.left - u * 2 / 10, y: r.top - h + u / 6 };
        let _ = ClientToScreen(main, &mut pt);
        pt.y = pt.y.max(0);
        render_flag(s.flag, text, w, h, Some(pt));
        let _ = SetWindowPos(
            s.flag,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

unsafe extern "system" fn flag_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_MOUSEACTIVATE {
            return LRESULT(MA_NOACTIVATE as isize);
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

unsafe fn paint(hwnd: HWND) {
    unsafe {
        let s = ST.get();
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let (cw, ch) = (rc.right - rc.left, rc.bottom - rc.top);

        let mem = CreateCompatibleDC(Some(hdc));
        let bmp = CreateCompatibleBitmap(hdc, cw, ch);
        let old_bmp = SelectObject(mem, bmp.into());
        SetBkMode(mem, TRANSPARENT);
        let old_pen = SelectObject(mem, GetStockObject(NULL_PEN));

        let b_bg = CreateSolidBrush(rgb(24, 26, 31));
        FillRect(mem, &rc, b_bg);

        // Title strip, which doubles as the drag handle.
        let mut title = RECT { left: 0, top: 0, right: cw, bottom: title_h(s.unit) };
        let old_font = SelectObject(mem, s.font_small.into());
        SetTextColor(mem, rgb(120, 126, 140));
        let mut text: Vec<u16> = "drag here   -   Mouse Keyboard".encode_utf16().collect();
        DrawTextW(mem, &mut text, &mut title, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

        let gap = (s.unit / 18).max(2);
        for (i, (r, key)) in placed(s.layout, s.unit).into_iter().enumerate() {
            let active = match key.act {
                Act::Shift => s.mods.shift,
                Act::Ctrl => s.mods.ctrl,
                Act::Alt => s.mods.alt,
                Act::Caps => s.caps,
                _ => false,
            };
            let (br, bg, bb) = if active {
                (46, 112, 214)
            } else if key.act == Act::Vk && key.label.chars().count() == 1 {
                (58, 62, 72)
            } else {
                (42, 45, 53)
            };
            let level = if s.pressed == Some(i) { 255 } else { s.fades.get(i).copied().unwrap_or(0) } as u32;
            let mix = |base: u32, hi: u32| (base * (255 - level) + hi * level) / 255;
            let brush = CreateSolidBrush(rgb(mix(br, 120), mix(bg, 150), mix(bb, 220)));
            SelectObject(mem, brush.into());
            let radius = s.unit / 6;
            let _ = RoundRect(
                mem,
                r.left + gap,
                r.top + gap,
                r.right - gap,
                r.bottom - gap,
                radius,
                radius,
            );
            SelectObject(mem, b_bg.into());
            let _ = DeleteObject(brush.into());

            let shifted = if key.is_letter() { s.mods.shift != s.caps } else { s.mods.shift };
            let label = if shifted { key.shifted } else { key.label };
            let single = label.chars().count() == 1;
            SelectObject(mem, if single { s.font_big } else { s.font_small }.into());
            SetTextColor(mem, rgb(235, 238, 245));
            let mut text: Vec<u16> = label.encode_utf16().collect();
            let mut tr = r;
            DrawTextW(mem, &mut text, &mut tr, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
        }

        let _ = BitBlt(hdc, 0, 0, cw, ch, Some(mem), 0, 0, SRCCOPY);

        SelectObject(mem, old_font);
        SelectObject(mem, old_pen);
        SelectObject(mem, old_bmp);
        for b in [b_bg] {
            let _ = DeleteObject(b.into());
        }
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        let _ = EndPaint(hwnd, &ps);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        let x = (lparam.0 & 0xFFFF) as i16 as i32;
        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
        match msg {
            WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
            WM_ERASEBKGND => return LRESULT(1),
            WM_PAINT => {
                paint(hwnd);
                return LRESULT(0);
            }
            WM_NCHITTEST => {
                let mut pt = POINT { x, y };
                let _ = ScreenToClient(hwnd, &mut pt);
                let unit = ST.get().unit;
                return LRESULT(if pt.y >= 0 && pt.y < title_h(unit) { HTCAPTION } else { HTCLIENT } as isize);
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                on_button_down(hwnd, x, y);
                return LRESULT(0);
            }
            WM_LBUTTONUP => {
                on_button_up(hwnd);
                return LRESULT(0);
            }
            WM_TIMER if wparam.0 == TIMER_REPEAT => {
                match ST.get().repeat {
                    Some((vk, mods)) => {
                        tap(vk, mods);
                        SetTimer(Some(hwnd), TIMER_REPEAT, REPEAT_RATE_MS, None);
                    }
                    None => {
                        let _ = KillTimer(Some(hwnd), TIMER_REPEAT);
                    }
                }
                return LRESULT(0);
            }
            WM_TIMER if wparam.0 == TIMER_LONG => {
                let _ = KillTimer(Some(hwnd), TIMER_LONG);
                let mut s = ST.get();
                if let Some((vk, mut mods)) = s.long.take() {
                    mods.shift = !mods.shift;
                    tap(vk, mods);
                    if s.flag_on {
                        let idx = if s.flag == s.flag_pool[1] { 1 } else { 0 };
                        s.flag_texts[idx] = s.long_text;
                        render_flag(s.flag, s.long_text, s.flag_size.0, s.flag_size.1, None);
                    }
                }
                ST.set(s);
                return LRESULT(0);
            }
            WM_TIMER if wparam.0 == TIMER_FADE => {
                let mut s = ST.get();
                let mut any = false;
                for (i, f) in s.fades.iter_mut().enumerate() {
                    if s.pressed != Some(i) && *f > 0 {
                        *f = f.saturating_sub(FADE_STEP);
                    }
                    any |= *f > 0;
                }
                ST.set(s);
                if !any {
                    let _ = KillTimer(Some(hwnd), TIMER_FADE);
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
                return LRESULT(0);
            }
            WM_TIMER if wparam.0 == TIMER_FLAG => {
                // No fade: the flag just disappears shortly after the key is released.
                let _ = KillTimer(Some(hwnd), TIMER_FLAG);
                let mut s = ST.get();
                let _ = ShowWindow(s.flag, SW_HIDE);
                s.flag_on = false;
                ST.set(s);
                return LRESULT(0);
            }
            WM_EXITSIZEMOVE => {
                let mut r = RECT::default();
                let _ = GetWindowRect(hwnd, &mut r);
                reg_set(w!("X"), r.left as u32);
                reg_set(w!("Y"), r.top as u32);
                return LRESULT(0);
            }
            WM_SHOW_KEYBOARD => {
                show_keyboard(hwnd);
                return LRESULT(0);
            }
            WM_TRAY => {
                match (lparam.0 & 0xFFFF) as u32 {
                    WM_LBUTTONUP => {
                        if is_shown(hwnd) {
                            let _ = ShowWindow(hwnd, SW_SHOWMINNOACTIVE);
                        } else {
                            show_keyboard(hwnd);
                        }
                    }
                    WM_RBUTTONUP => on_tray_menu(hwnd),
                    _ => {}
                }
                return LRESULT(0);
            }
            WM_DESTROY => {
                remove_icon(hwnd);
                set_hot_corner(false);
                PostQuitMessage(0);
                return LRESULT(0);
            }
            _ => {
                let tb = ST.get().taskbar_created;
                if tb != 0 && msg == tb {
                    add_icon(hwnd);
                    return LRESULT(0);
                }
            }
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}
