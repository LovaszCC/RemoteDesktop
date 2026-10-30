//! Whether the embedded RDP swaps the key left of `1` with the key next to left Shift.
//!
//! On Mac ISO keyboards (Hungarian, German…) macOS reports those two keys the other way round than a PC
//! does, so `WebKit`'s `Backquote` and `IntlBackslash` reach the server swapped (a Hungarian layout types
//! `í` for `0`). The swap setting puts them back; unset, it follows the hardware: on when a keyboard is
//! ISO. Only macOS swaps.

/// The swap in effect for the `swapIsoKeys` setting (`None` = automatic).
#[must_use]
pub fn swap_iso_keys(setting: Option<bool>) -> bool {
    resolve(cfg!(target_os = "macos"), setting, is_iso_keyboard)
}

/// The rule: never off macOS; the setting if set; else what `detect` says.
fn resolve(mac: bool, setting: Option<bool>, detect: impl FnOnce() -> bool) -> bool {
    mac && setting.unwrap_or_else(detect)
}

/// Whether a connected keyboard is ISO (its `IOHIDEventDriver` has `StandardType` 1). `false` off macOS
/// or when no keyboard says.
#[must_use]
pub fn is_iso_keyboard() -> bool {
    #[cfg(target_os = "macos")]
    {
        mac::standard_types().contains(&mac::ISO)
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod mac {
    //! `IOKit` registry lookups: C functions over Core Foundation objects, hence the `unsafe` blocks.

    use std::ffi::{c_char, c_void};
    use std::ptr::NonNull;

    use objc2_core_foundation::{CFNumber, CFRetained, CFString, CFType};

    /// `StandardType` of an ISO keyboard (0 = ANSI, 2 = JIS).
    pub const ISO: i64 = 1;

    type IoObject = u32;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOServiceMatching(name: *const c_char) -> *mut c_void;
        fn IOServiceGetMatchingServices(
            main_port: u32,
            matching: *mut c_void,
            existing: *mut IoObject,
        ) -> i32;
        fn IOIteratorNext(iterator: IoObject) -> IoObject;
        fn IOObjectRelease(object: IoObject) -> i32;
        fn IORegistryEntryCreateCFProperty(
            entry: IoObject,
            key: &CFString,
            allocator: *const c_void,
            options: u32,
        ) -> Option<NonNull<CFType>>;
    }

    /// The `StandardType` of every HID event driver that has one.
    pub fn standard_types() -> Vec<i64> {
        let mut types = Vec::new();
        // SAFETY: a NUL-terminated class name; the dictionary it returns is consumed by the next call.
        let matching = unsafe { IOServiceMatching(c"IOHIDEventDriver".as_ptr()) };
        if matching.is_null() {
            return types;
        }
        let mut iterator: IoObject = 0;
        // SAFETY: port 0 is the default main port; `iterator` is written on success.
        if unsafe { IOServiceGetMatchingServices(0, matching, &raw mut iterator) } != 0 {
            return types;
        }
        let key = CFString::from_static_str("StandardType");
        loop {
            // SAFETY: a valid iterator; 0 ends it.
            let service = unsafe { IOIteratorNext(iterator) };
            if service == 0 {
                break;
            }
            // SAFETY: a valid registry entry; the property comes back retained (Create rule).
            let value = unsafe { IORegistryEntryCreateCFProperty(service, &key, std::ptr::null(), 0) }
                .map(|ptr| unsafe { CFRetained::from_raw(ptr) });
            if let Some(number) = value.and_then(|v| v.downcast::<CFNumber>().ok())
                && let Some(kind) = number.as_i64()
            {
                types.push(kind);
            }
            // SAFETY: each object from the iterator is released once.
            unsafe { IOObjectRelease(service) };
        }
        // SAFETY: the iterator is released once.
        unsafe { IOObjectRelease(iterator) };
        types
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_macos_it_never_swaps() {
        assert!(!resolve(false, Some(true), || true));
        assert!(!resolve(false, None, || true));
    }

    #[test]
    fn the_setting_wins_over_detection() {
        assert!(resolve(true, Some(true), || false));
        assert!(!resolve(true, Some(false), || true));
    }

    #[test]
    fn unset_follows_detection() {
        assert!(resolve(true, None, || true));
        assert!(!resolve(true, None, || false));
    }

    #[test]
    fn detection_does_not_panic() {
        let iso = is_iso_keyboard();
        if !cfg!(target_os = "macos") {
            assert!(!iso);
        }
        assert!(!swap_iso_keys(Some(false)));
    }
}
