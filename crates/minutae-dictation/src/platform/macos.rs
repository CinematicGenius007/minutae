//! macOS: Accessibility trust, focused-element role, frontmost app.

use std::ffi::c_void;

use core_foundation::{
    base::{CFType, CFTypeRef, TCFType},
    boolean::CFBoolean,
    dictionary::CFDictionary,
    string::{CFString, CFStringRef},
};

use crate::insert::{CopyReason, Preflight};

type AXUIElementRef = *const c_void;
type AXError = i32;
const AX_SUCCESS: AXError = 0;
const AX_ATTRIBUTE_UNSUPPORTED: AXError = -25205;
const AX_NO_VALUE: AXError = -25212;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
    fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut i32) -> AXError;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, seconds: f32) -> AXError;
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
}

/// Whether this process may post keystrokes and read the focused element.
pub fn accessibility_trusted() -> bool {
    // SAFETY: no arguments; returns a Boolean.
    unsafe { AXIsProcessTrusted() != 0 }
}

/// Shows the system "allow Accessibility" prompt if not yet trusted.
/// Returns the current trust state (the user answers asynchronously).
pub fn prompt_accessibility() -> bool {
    // SAFETY: kAXTrustedCheckOptionPrompt is a valid static CFString owned by
    // the framework; wrap_under_get_rule retains it.
    let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let options =
        CFDictionary::from_CFType_pairs(&[(key.as_CFType(), CFBoolean::true_value().as_CFType())]);
    // SAFETY: options is a valid CFDictionary for the duration of the call.
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef() as *const c_void) != 0 }
}

pub fn frontmost_app_name() -> Option<String> {
    let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    app.localizedName().map(|name| name.to_string())
}

struct Focused {
    pid: i32,
    role: Option<String>,
    subrole: Option<String>,
}

fn copy_string_attribute(
    element: AXUIElementRef,
    name: &'static str,
) -> Result<Option<String>, ()> {
    let attribute = CFString::from_static_string(name);
    let mut value: CFTypeRef = std::ptr::null();
    // SAFETY: element is a live AXUIElement; value receives a +1 reference on success.
    let error = unsafe {
        AXUIElementCopyAttributeValue(element, attribute.as_concrete_TypeRef(), &mut value)
    };
    if matches!(error, AX_ATTRIBUTE_UNSUPPORTED | AX_NO_VALUE) {
        return Ok(None);
    }
    if error != AX_SUCCESS || value.is_null() {
        return Err(());
    }
    // SAFETY: Copy rule: we own `value`.
    let value = unsafe { CFType::wrap_under_create_rule(value) };
    value
        .downcast::<CFString>()
        .map(|s| Some(s.to_string()))
        .ok_or(())
}

/// Role and subrole of the system-wide focused element, or `None` when it
/// cannot be read (no focus, or an app that does not expose Accessibility).
fn focused_element() -> Option<Focused> {
    // SAFETY: creates a +1 system-wide element that we release below.
    let system = unsafe { AXUIElementCreateSystemWide() };
    if system.is_null() {
        return None;
    }
    // A hung target app would otherwise block for ~6 s.
    // SAFETY: system is a valid AXUIElement.
    unsafe { AXUIElementSetMessagingTimeout(system, 0.25) };
    let attribute = CFString::from_static_string("AXFocusedUIElement");
    let mut focused: CFTypeRef = std::ptr::null();
    // SAFETY: as in copy_string_attribute.
    let error = unsafe {
        AXUIElementCopyAttributeValue(system, attribute.as_concrete_TypeRef(), &mut focused)
    };
    // SAFETY: balance AXUIElementCreateSystemWide.
    unsafe { CFRelease(system) };
    if error != AX_SUCCESS || focused.is_null() {
        return None;
    }
    let mut pid = 0;
    // SAFETY: focused is a live AXUIElement and pid is writable.
    unsafe { AXUIElementGetPid(focused, &mut pid) };
    let role = copy_string_attribute(focused, "AXRole");
    let subrole = copy_string_attribute(focused, "AXSubrole");
    // SAFETY: we own the +1 focused element.
    unsafe { CFRelease(focused) };
    Some(Focused {
        pid,
        role: role.ok()?,
        subrole: subrole.ok()?,
    })
}

pub(crate) fn preflight(process: u32) -> Preflight {
    if !accessibility_trusted() {
        return Preflight::CopyOnly(CopyReason::AccessibilityNotTrusted);
    }
    if secure_input() {
        return Preflight::CopyOnly(CopyReason::SecureInput);
    }
    let Some(focused) = focused_element() else {
        return Preflight::CopyOnly(CopyReason::UnverifiedField);
    };
    if focused.pid as u32 != process {
        return Preflight::CopyOnly(CopyReason::FocusChanged);
    }
    if [focused.role.as_deref(), focused.subrole.as_deref()].contains(&Some("AXSecureTextField")) {
        return Preflight::CopyOnly(CopyReason::SecureInput);
    }
    if !matches!(
        focused.role.as_deref(),
        Some("AXTextField" | "AXTextArea" | "AXComboBox")
    ) {
        return Preflight::CopyOnly(CopyReason::UnverifiedField);
    }
    Preflight::Paste
}

fn secure_input() -> bool {
    // SAFETY: no arguments.
    unsafe { IsSecureEventInputEnabled() != 0 }
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, down: bool) -> *const c_void;
    fn CGEventSetFlags(event: *const c_void, flags: u64);
    fn CGEventPostToPid(pid: i32, event: *const c_void);
}

/// Send only a command key combination, to the checked PID even if another
/// app receives focus in the interval between the check and the dispatch.
pub(crate) fn frontmost_launch_time(process: u32) -> Option<u64> {
    let app = objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication()?;
    if app.processIdentifier() as u32 != process {
        return None;
    }
    Some(app.launchDate()?.timeIntervalSince1970().to_bits())
}

pub(crate) fn send_command(
    process: u32,
    key: u16,
    launched_at: Option<u64>,
) -> Result<(), CopyReason> {
    use super::identity::{AppIdentityLookup, SystemAppIdentityLookup};
    const COMMAND: u64 = 1 << 20;
    // SAFETY: create two owned events for the given virtual keycode; a null
    // source requests the default. Release each successful create exactly once.
    unsafe {
        let down = CGEventCreateKeyboardEvent(std::ptr::null(), key, true);
        let up = CGEventCreateKeyboardEvent(std::ptr::null(), key, false);
        let result = if down.is_null() || up.is_null() {
            Err(CopyReason::PasteFailed)
        } else if launched_at.map_or_else(
            || SystemAppIdentityLookup.foreground_target().1 != Some(process),
            |time| frontmost_launch_time(process) != Some(time),
        ) {
            Err(CopyReason::FocusChanged)
        } else if secure_input() {
            Err(CopyReason::SecureInput)
        } else {
            for event in [down, up] {
                CGEventSetFlags(event, COMMAND);
                CGEventPostToPid(process as i32, event);
            }
            Ok(())
        };
        for event in [down, up] {
            if !event.is_null() {
                CFRelease(event);
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Paste keycode for the current keyboard layout
// ---------------------------------------------------------------------------

/// `kVK_ANSI_V`: the "v" key on a QWERTY layout.
pub(crate) const ANSI_V: u16 = 9;

type TISInputSourceRef = *const c_void;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn TISCopyCurrentKeyboardLayoutInputSource() -> TISInputSourceRef;
    fn TISGetInputSourceProperty(source: TISInputSourceRef, key: CFStringRef) -> *const c_void;
    static kTISPropertyUnicodeKeyLayoutData: CFStringRef;
    fn LMGetKbdType() -> u8;
    fn UCKeyTranslate(
        key_layout: *const c_void,
        virtual_key_code: u16,
        key_action: u16,
        modifier_key_state: u32,
        keyboard_type: u32,
        key_translate_options: u32,
        dead_key_state: *mut u32,
        max_string_length: usize,
        actual_string_length: *mut usize,
        unicode_string: *mut u16,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
}

unsafe extern "C" {
    fn pthread_main_np() -> i32;
}

/// Whether the caller runs on the process's main thread.
pub(crate) fn is_main_thread() -> bool {
    // SAFETY: no arguments, no side effects.
    unsafe { pthread_main_np() != 0 }
}

/// The virtual keycode that produces a letter together with ⌘ in the current keyboard layout
/// (e.g. 47 on Dvorak, 9 on QWERTY and "Dvorak – QWERTY ⌘"). `None` when the layout has no
/// Unicode layout data or no key maps to "v".
///
/// **Main thread only**: Text Input Source Services assert the main queue on recent macOS
/// and abort the process otherwise. Callers check [`is_main_thread`] first.
pub(crate) fn lookup_keycode(key: &str) -> Option<u16> {
    const KUC_KEY_ACTION_DISPLAY: u16 = 3;
    const NO_DEAD_KEYS: u32 = 1; // 1 << kUCKeyTranslateNoDeadKeysBit
    const CMD_KEY_STATE: u32 = (0x0100 >> 8) & 0xFF; // (cmdKey >> 8) & 0xFF
    // SAFETY: TIS returns a +1 input source we release; the layout data is a CFData owned by
    // that source and only read while it is alive; UCKeyTranslate writes at most 4 UTF-16
    // units into `buf`.
    unsafe {
        let source = TISCopyCurrentKeyboardLayoutInputSource();
        if source.is_null() {
            return None;
        }
        let data = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData);
        let found = if data.is_null() {
            None
        } else {
            let layout = CFDataGetBytePtr(data) as *const c_void;
            let kbd_type = u32::from(LMGetKbdType());
            let translate = |code: u16, modifiers: u32| -> Option<String> {
                let mut dead = 0u32;
                let mut len = 0usize;
                let mut buf = [0u16; 4];
                let status = UCKeyTranslate(
                    layout,
                    code,
                    KUC_KEY_ACTION_DISPLAY,
                    modifiers,
                    kbd_type,
                    NO_DEAD_KEYS,
                    &mut dead,
                    buf.len(),
                    &mut len,
                    buf.as_mut_ptr(),
                );
                (status == 0 && len > 0).then(|| String::from_utf16_lossy(&buf[..len.min(4)]))
            };
            // With ⌘ held first (honours "… – QWERTY ⌘" layouts), then the plain layer.
            [CMD_KEY_STATE, 0].into_iter().find_map(|mods| {
                (0u16..128).find(|&code| {
                    translate(code, mods).is_some_and(|s| s.eq_ignore_ascii_case(key))
                })
            })
        };
        CFRelease(source);
        found
    }
}
