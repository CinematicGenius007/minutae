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

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
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

/// Roles that clearly cannot take typed text. Anything else, including
/// web areas, groups and unknown roles (Electron, browsers), is attempted.
const NON_TEXT_ROLES: &[&str] = &[
    "AXButton",
    "AXCheckBox",
    "AXRadioButton",
    "AXMenuItem",
    "AXMenuBarItem",
    "AXMenuButton",
    "AXPopUpButton",
    "AXImage",
    "AXWindow",
    "AXApplication",
    "AXList",
    "AXTable",
    "AXOutline",
    "AXRow",
    "AXCell",
    "AXSlider",
    "AXLink",
    "AXTabGroup",
    "AXToolbar",
    "AXDisclosureTriangle",
    "AXStaticText",
    "AXDockItem",
];

struct Focused {
    role: Option<String>,
    subrole: Option<String>,
}

fn copy_string_attribute(element: AXUIElementRef, name: &'static str) -> Option<String> {
    let attribute = CFString::from_static_string(name);
    let mut value: CFTypeRef = std::ptr::null();
    // SAFETY: element is a live AXUIElement; value receives a +1 reference on success.
    let error = unsafe {
        AXUIElementCopyAttributeValue(element, attribute.as_concrete_TypeRef(), &mut value)
    };
    if error != AX_SUCCESS || value.is_null() {
        return None;
    }
    // SAFETY: Copy rule: we own `value`.
    let value = unsafe { CFType::wrap_under_create_rule(value) };
    value.downcast::<CFString>().map(|s| s.to_string())
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
    let result = Focused {
        role: copy_string_attribute(focused, "AXRole"),
        subrole: copy_string_attribute(focused, "AXSubrole"),
    };
    // SAFETY: we own the +1 focused element.
    unsafe { CFRelease(focused) };
    Some(result)
}

pub(crate) fn preflight() -> Preflight {
    if !accessibility_trusted() {
        return Preflight::CopyOnly(CopyReason::AccessibilityNotTrusted);
    }
    // SAFETY: no arguments.
    if unsafe { IsSecureEventInputEnabled() } != 0 {
        return Preflight::CopyOnly(CopyReason::SecureInput);
    }
    if let Some(focused) = focused_element() {
        if focused.subrole.as_deref() == Some("AXSecureTextField") {
            return Preflight::Refuse("Won't type into a password field".into());
        }
        if focused
            .role
            .as_deref()
            .is_some_and(|role| NON_TEXT_ROLES.contains(&role))
        {
            return Preflight::CopyOnly(CopyReason::NoTextField);
        }
    }
    Preflight::Paste
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

/// The virtual keycode that produces "v" together with ⌘ in the current keyboard layout
/// (e.g. 47 on Dvorak, 9 on QWERTY and "Dvorak – QWERTY ⌘"). `None` when the layout has no
/// Unicode layout data or no key maps to "v".
///
/// **Main thread only**: Text Input Source Services assert the main queue on recent macOS
/// and abort the process otherwise. Callers check [`is_main_thread`] first.
pub(crate) fn lookup_v_keycode() -> Option<u16> {
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
                    translate(code, mods).is_some_and(|s| s.eq_ignore_ascii_case("v"))
                })
            })
        };
        CFRelease(source);
        found
    }
}
