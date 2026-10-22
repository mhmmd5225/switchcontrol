use windows::Win32::Foundation::{LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CURSORINFO, GetCursorInfo, GetCursorPos, GetSystemMetrics, MONITORINFOF_PRIMARY,
    SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};
use windows::core::BOOL;

use crate::{Monitor, Rect};

/// Bounding box of all monitors, in physical pixels.
pub fn virtual_desktop() -> Rect {
    unsafe {
        let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        Rect {
            left,
            top,
            right: left + GetSystemMetrics(SM_CXVIRTUALSCREEN),
            bottom: top + GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}

pub fn monitors() -> Vec<Monitor> {
    let mut out: Vec<Monitor> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(enum_monitor),
            LPARAM(&mut out as *mut Vec<Monitor> as isize),
        );
    }
    out
}

pub fn cursor_pos() -> Option<(i32, i32)> {
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p) }.ok().map(|_| (p.x, p.y))
}

/// Identifies the cursor image currently shown, for tests and diagnostics.
pub fn cursor_image() -> Option<isize> {
    let mut info = CURSORINFO { cbSize: size_of::<CURSORINFO>() as u32, ..Default::default() };
    unsafe { GetCursorInfo(&mut info) }.ok().map(|_| info.hCursor.0 as isize)
}

unsafe extern "system" fn enum_monitor(
    hmon: HMONITOR,
    _hdc: HDC,
    _clip: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let out = unsafe { &mut *(data.0 as *mut Vec<Monitor>) };
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    if unsafe { GetMonitorInfoW(hmon, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO) }
        .as_bool()
    {
        let r = info.monitorInfo.rcMonitor;
        let len = info.szDevice.iter().position(|&c| c == 0).unwrap_or(info.szDevice.len());
        out.push(Monitor {
            name: String::from_utf16_lossy(&info.szDevice[..len]),
            bounds: Rect { left: r.left, top: r.top, right: r.right, bottom: r.bottom },
            primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
    }
    true.into()
}
