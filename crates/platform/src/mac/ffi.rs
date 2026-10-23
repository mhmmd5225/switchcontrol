//! The small slice of CoreGraphics / CoreFoundation / Carbon this backend uses.

#![allow(non_upper_case_globals, non_snake_case, dead_code)]

use std::ffi::c_void;

pub type CFTypeRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFMachPortRef = *mut c_void;
pub type CFRunLoopRef = *mut c_void;
pub type CFRunLoopSourceRef = *mut c_void;
pub type CGEventRef = *mut c_void;
pub type CGEventSourceRef = *mut c_void;
pub type CGEventTapProxy = *mut c_void;
pub type CGDirectDisplayID = u32;
pub type CGEventType = u32;
pub type CGEventField = u32;
pub type CGEventFlags = u64;
pub type CGError = i32;
pub type CGEventSourceStateID = i32;

pub const kCGEventSourceStateCombinedSessionState: CGEventSourceStateID = 0;
pub const kCGEventSourceStateHIDSystemState: CGEventSourceStateID = 1;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

pub type CGEventTapCallBack = unsafe extern "C" fn(
    proxy: CGEventTapProxy,
    ty: CGEventType,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef;

// CGEventType
pub const kCGEventLeftMouseDown: CGEventType = 1;
pub const kCGEventLeftMouseUp: CGEventType = 2;
pub const kCGEventRightMouseDown: CGEventType = 3;
pub const kCGEventRightMouseUp: CGEventType = 4;
pub const kCGEventMouseMoved: CGEventType = 5;
pub const kCGEventLeftMouseDragged: CGEventType = 6;
pub const kCGEventRightMouseDragged: CGEventType = 7;
pub const kCGEventKeyDown: CGEventType = 10;
pub const kCGEventKeyUp: CGEventType = 11;
pub const kCGEventFlagsChanged: CGEventType = 12;
pub const kCGEventScrollWheel: CGEventType = 22;
pub const kCGEventOtherMouseDown: CGEventType = 25;
pub const kCGEventOtherMouseUp: CGEventType = 26;
pub const kCGEventOtherMouseDragged: CGEventType = 27;
pub const kCGEventTapDisabledByTimeout: CGEventType = 0xFFFF_FFFE;
pub const kCGEventTapDisabledByUserInput: CGEventType = 0xFFFF_FFFF;

// CGEventField
pub const kCGMouseEventClickState: CGEventField = 1;
pub const kCGMouseEventButtonNumber: CGEventField = 3;
pub const kCGMouseEventDeltaX: CGEventField = 4;
pub const kCGMouseEventDeltaY: CGEventField = 5;
pub const kCGKeyboardEventKeycode: CGEventField = 9;
pub const kCGEventSourceUserData: CGEventField = 42;
pub const kCGScrollWheelEventFixedPtDeltaAxis1: CGEventField = 93;
pub const kCGScrollWheelEventFixedPtDeltaAxis2: CGEventField = 94;

// CGEventFlags: device-independent
pub const kCGEventFlagMaskAlphaShift: CGEventFlags = 0x0001_0000;
pub const kCGEventFlagMaskShift: CGEventFlags = 0x0002_0000;
pub const kCGEventFlagMaskControl: CGEventFlags = 0x0004_0000;
pub const kCGEventFlagMaskAlternate: CGEventFlags = 0x0008_0000;
pub const kCGEventFlagMaskCommand: CGEventFlags = 0x0010_0000;
pub const kCGEventFlagMaskNumericPad: CGEventFlags = 0x0020_0000;
pub const kCGEventFlagMaskSecondaryFn: CGEventFlags = 0x0080_0000;
// CGEventFlags: device-dependent (IOKit NX_DEVICE*KEYMASK), tell left from right
pub const NX_DEVICELCTLKEYMASK: CGEventFlags = 0x0000_0001;
pub const NX_DEVICELSHIFTKEYMASK: CGEventFlags = 0x0000_0002;
pub const NX_DEVICERSHIFTKEYMASK: CGEventFlags = 0x0000_0004;
pub const NX_DEVICELCMDKEYMASK: CGEventFlags = 0x0000_0008;
pub const NX_DEVICERCMDKEYMASK: CGEventFlags = 0x0000_0010;
pub const NX_DEVICELALTKEYMASK: CGEventFlags = 0x0000_0020;
pub const NX_DEVICERALTKEYMASK: CGEventFlags = 0x0000_0040;
pub const NX_DEVICERCTLKEYMASK: CGEventFlags = 0x0000_2000;

// CGEventTapLocation / placement / options
pub const kCGHIDEventTap: u32 = 0;
pub const kCGSessionEventTap: u32 = 1;
pub const kCGHeadInsertEventTap: u32 = 0;
pub const kCGEventTapOptionDefault: u32 = 0;

pub const kCFStringEncodingUTF8: u32 = 0x0800_0100;

// CGScrollEventUnit
pub const kCGScrollEventUnitLine: u32 = 1;

// CGMouseButton
pub const kCGMouseButtonLeft: u32 = 0;
pub const kCGMouseButtonRight: u32 = 1;
pub const kCGMouseButtonCenter: u32 = 2;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    pub fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: CGEventTapCallBack,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    pub fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);

    pub fn CGEventCreate(source: CGEventSourceRef) -> CGEventRef;
    pub fn CGEventCreateMouseEvent(
        source: CGEventSourceRef,
        mouse_type: CGEventType,
        location: CGPoint,
        button: u32,
    ) -> CGEventRef;
    pub fn CGEventCreateKeyboardEvent(
        source: CGEventSourceRef,
        keycode: u16,
        key_down: bool,
    ) -> CGEventRef;
    pub fn CGEventCreateScrollWheelEvent2(
        source: CGEventSourceRef,
        units: u32,
        wheel_count: u32,
        wheel1: i32,
        wheel2: i32,
        wheel3: i32,
    ) -> CGEventRef;
    pub fn CGEventPost(tap: u32, event: CGEventRef);

    pub fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    pub fn CGEventGetIntegerValueField(event: CGEventRef, field: CGEventField) -> i64;
    pub fn CGEventSetIntegerValueField(event: CGEventRef, field: CGEventField, value: i64);
    pub fn CGEventGetFlags(event: CGEventRef) -> CGEventFlags;
    pub fn CGEventSetFlags(event: CGEventRef, flags: CGEventFlags);
    pub fn CGEventSetType(event: CGEventRef, ty: CGEventType);

    pub fn CGAssociateMouseAndMouseCursorPosition(connected: u32) -> CGError;
    pub fn CGGetActiveDisplayList(
        max_displays: u32,
        displays: *mut CGDirectDisplayID,
        count: *mut u32,
    ) -> CGError;
    pub fn CGDisplayBounds(display: CGDirectDisplayID) -> CGRect;
    pub fn CGMainDisplayID() -> CGDirectDisplayID;

    pub fn CGRequestListenEventAccess() -> bool;
    pub fn CGRequestPostEventAccess() -> bool;
    pub fn CGPreflightListenEventAccess() -> bool;
    pub fn CGEventSourceKeyState(state: CGEventSourceStateID, key: u16) -> bool;
    pub fn CGPreflightPostEventAccess() -> bool;

    pub fn CGDisplayHideCursor(display: CGDirectDisplayID) -> CGError;
    pub fn CGDisplayShowCursor(display: CGDirectDisplayID) -> CGError;

    // Private but long-stable WindowServer calls, also used by Synergy,
    // Barrier and Deskflow: they let a background process hide the cursor.
    pub fn _CGSDefaultConnection() -> i32;
    pub fn CGSSetConnectionProperty(
        connection: i32,
        target: i32,
        key: CFStringRef,
        value: CFTypeRef,
    ) -> CGError;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub static kCFRunLoopCommonModes: CFStringRef;
    pub static kCFRunLoopDefaultMode: CFStringRef;
    pub static kCFBooleanTrue: CFTypeRef;

    pub fn CFStringCreateWithCString(
        allocator: CFTypeRef,
        text: *const std::ffi::c_char,
        encoding: u32,
    ) -> CFStringRef;

    pub fn CFMachPortCreateRunLoopSource(
        allocator: CFTypeRef,
        port: CFMachPortRef,
        order: isize,
    ) -> CFRunLoopSourceRef;
    pub fn CFMachPortInvalidate(port: CFMachPortRef);
    pub fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    pub fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    pub fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, return_after_source: u8) -> i32;
    pub fn CFRelease(cf: CFTypeRef);
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    pub fn IsSecureEventInputEnabled() -> u8;
}
