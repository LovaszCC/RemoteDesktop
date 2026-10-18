//! Types the password into Windows App's credential prompt through the Accessibility API (Swift
//! `AccessibilityAutofill`): poll every 300 ms for up to 30 s for an empty secure text field in the app's
//! windows (depth ≤ 15), set its value, then press the default button, a submit button by title, or confirm
//! the field.
//!
//! The Accessibility calls are C functions over Core Foundation objects, hence the `unsafe` blocks; each one
//! says why it is sound. The tree search itself is [`super::ax_tree`].
#![allow(unsafe_code)]

use std::ptr::NonNull;
use std::time::{Duration, Instant};

use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
use objc2_application_services::{
    AXError, AXIsProcessTrustedWithOptions, AXUIElement, kAXTrustedCheckOptionPrompt,
};
use objc2_core_foundation::{CFArray, CFBoolean, CFDictionary, CFRetained, CFString, CFType};
use objc2_foundation::NSString;

use super::ax_tree::{self, AxNode};

/// Pause between polls.
pub const POLL_INTERVAL: Duration = Duration::from_millis(300);
/// How long to wait for the prompt.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// Whether this process may use the Accessibility API; `prompt` shows the system's request once.
#[must_use]
pub fn is_trusted(prompt: bool) -> bool {
    // SAFETY: `kAXTrustedCheckOptionPrompt` is an immutable CFString constant exported by the framework.
    let key: &CFString = unsafe { kAXTrustedCheckOptionPrompt };
    let value: &CFType = CFBoolean::new(prompt);
    let options = CFDictionary::<CFString, CFType>::from_slices(&[key], &[value]);
    // SAFETY: the dictionary maps the documented option key to a CFBoolean, as the function expects.
    unsafe { AXIsProcessTrustedWithOptions(Some(options.as_opaque())) }
}

/// An element with the attribute helpers.
struct Element(CFRetained<AXUIElement>);

impl Element {
    fn attribute(&self, name: &str) -> Option<CFRetained<CFType>> {
        let name = CFString::from_str(name);
        let mut value: *const CFType = std::ptr::null();
        // SAFETY: `value` is a valid out pointer for the call; the element and name are live CF objects.
        let error = unsafe { self.0.copy_attribute_value(&name, NonNull::from(&mut value)) };
        if error != AXError::Success {
            return None;
        }
        let value = NonNull::new(value.cast_mut())?;
        // SAFETY: a successful "Copy" call hands over one reference to a valid CF object, which
        // `CFRetained` now owns and releases.
        Some(unsafe { CFRetained::from_raw(value) })
    }

    fn string(&self, name: &str) -> Option<String> {
        self.attribute(name)?
            .downcast::<CFString>()
            .ok()
            .map(|s| s.to_string())
    }

    fn elements(&self, name: &str) -> Vec<Element> {
        let Some(array) = self.attribute(name).and_then(|v| v.downcast::<CFArray>().ok()) else {
            return Vec::new();
        };
        // SAFETY: the Accessibility API documents these attributes as arrays of AXUIElements; each item is
        // checked again with `downcast` below.
        let array: CFRetained<CFArray<CFType>> = unsafe { CFRetained::cast_unchecked(array) };
        array
            .iter()
            .filter_map(|item| item.downcast::<AXUIElement>().ok())
            .map(Element)
            .collect()
    }

    fn set(&self, name: &str, value: &CFType) -> bool {
        let name = CFString::from_str(name);
        // SAFETY: element, name and value are live CF objects of the types the call takes.
        unsafe { self.0.set_attribute_value(&name, value) == AXError::Success }
    }

    fn perform(&self, action: &str) {
        let action = CFString::from_str(action);
        // SAFETY: element and action name are live CF objects.
        let _ = unsafe { self.0.perform_action(&action) };
    }

    fn default_button(&self) -> Option<Element> {
        self.attribute("AXDefaultButton")?
            .downcast::<AXUIElement>()
            .ok()
            .map(Element)
    }
}

impl AxNode for Element {
    fn role(&self) -> Option<String> {
        self.string("AXRole")
    }
    fn subrole(&self) -> Option<String> {
        self.string("AXSubrole")
    }
    fn title(&self) -> Option<String> {
        self.string("AXTitle")
    }
    fn value(&self) -> Option<String> {
        self.string("AXValue")
    }
    fn children(&self) -> Vec<Self> {
        let mut children = self.elements("AXChildren");
        children.extend(self.elements("AXSheets"));
        children
    }
}

fn submit(password: &str, field: &Element, window: &Element, app: &NSRunningApplication) -> bool {
    let _ = app.activateWithOptions(NSApplicationActivationOptions::empty());
    let yes: &CFType = CFBoolean::new(true);
    window.set("AXMain", yes);
    field.set("AXFocused", yes);
    if !field.set("AXValue", &CFString::from_str(password)) {
        return false;
    }
    let window_copy = Element(window.0.clone());
    match window
        .default_button()
        .or_else(|| ax_tree::find_submit_button(window_copy, 0))
    {
        Some(button) => button.perform("AXPress"),
        None => field.perform("AXConfirm"),
    }
    true
}

/// Polls the app with `bundle_id` for up to `timeout`; `true` once the password was filled in and
/// submitted.
#[must_use]
pub fn fill_password(password: &str, bundle_id: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let apps =
            NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(bundle_id));
        if let Some(app) = apps.firstObject() {
            // SAFETY: creating the application element only needs a process id; a stale one yields an
            // element whose calls fail.
            let root = Element(unsafe { AXUIElement::new_application(app.processIdentifier()) });
            for window in root.elements("AXWindows") {
                let search = Element(window.0.clone());
                if let Some(field) = ax_tree::find_empty_secure_field(search, 0) {
                    return submit(password, &field, &window, &app);
                }
            }
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checking_the_permission_without_prompt_works() {
        // Either answer is fine: the call must just not crash.
        let _ = is_trusted(false);
    }

    #[test]
    fn gives_up_when_the_app_is_not_running() {
        assert!(!fill_password(
            "x",
            "hu.example.no-such-app",
            Duration::from_millis(10)
        ));
    }
}
