import AppKit
import ApplicationServices

/// Finds a password field in another app's windows and fills it via the Accessibility API.
@MainActor
enum AccessibilityAutofill {
    static let submitTitles: Set<String> = ["continue", "connect", "ok", "sign in", "folytatás", "csatlakozás", "bejelentkezés"]

    static func isTrusted(prompt: Bool) -> Bool {
        let options = ["AXTrustedCheckOptionPrompt": prompt] as CFDictionary
        return AXIsProcessTrustedWithOptions(options)
    }

    /// Polls the target app for up to `timeout` seconds; returns `true` once the password was submitted.
    static func fillPassword(
        _ password: String,
        bundleIdentifier: String,
        timeout: Double = 30
    ) async -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if let app = NSRunningApplication.runningApplications(withBundleIdentifier: bundleIdentifier).first {
                let root = AXUIElementCreateApplication(app.processIdentifier)
                for window in elements(root, kAXWindowsAttribute) {
                    if let field = findEmptySecureField(in: window, depth: 0) {
                        return submit(password, field: field, window: window, app: app)
                    }
                }
            }
            try? await Task.sleep(for: .milliseconds(300))
        }
        return false
    }

    private static func submit(_ password: String, field: AXUIElement, window: AXUIElement, app: NSRunningApplication) -> Bool {
        app.activate()
        AXUIElementSetAttributeValue(window, kAXMainAttribute as CFString, kCFBooleanTrue)
        AXUIElementSetAttributeValue(field, kAXFocusedAttribute as CFString, kCFBooleanTrue)
        guard AXUIElementSetAttributeValue(field, kAXValueAttribute as CFString, password as CFString) == .success else {
            return false
        }
        if let button = defaultButton(of: window) ?? findSubmitButton(in: window, depth: 0) {
            AXUIElementPerformAction(button, kAXPressAction as CFString)
        } else {
            AXUIElementPerformAction(field, kAXConfirmAction as CFString)
        }
        return true
    }

    // MARK: Tree search

    private static func findEmptySecureField(in element: AXUIElement, depth: Int) -> AXUIElement? {
        guard depth < 15 else { return nil }
        if string(element, kAXSubroleAttribute) == kAXSecureTextFieldSubrole as String {
            let value = string(element, kAXValueAttribute) ?? ""
            if value.isEmpty { return element }
        }
        for child in elements(element, kAXChildrenAttribute) + elements(element, "AXSheets") {
            if let found = findEmptySecureField(in: child, depth: depth + 1) { return found }
        }
        return nil
    }

    private static func findSubmitButton(in element: AXUIElement, depth: Int) -> AXUIElement? {
        guard depth < 15 else { return nil }
        if string(element, kAXRoleAttribute) == kAXButtonRole as String,
           let title = string(element, kAXTitleAttribute)?.lowercased(),
           submitTitles.contains(title) {
            return element
        }
        for child in elements(element, kAXChildrenAttribute) + elements(element, "AXSheets") {
            if let found = findSubmitButton(in: child, depth: depth + 1) { return found }
        }
        return nil
    }

    private static func defaultButton(of window: AXUIElement) -> AXUIElement? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(window, kAXDefaultButtonAttribute as CFString, &value) == .success,
              let value, CFGetTypeID(value) == AXUIElementGetTypeID()
        else { return nil }
        return (value as! AXUIElement)
    }

    // MARK: Attribute helpers

    private static func string(_ element: AXUIElement, _ attribute: String) -> String? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(element, attribute as CFString, &value) == .success else { return nil }
        return value as? String
    }

    private static func elements(_ element: AXUIElement, _ attribute: String) -> [AXUIElement] {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(element, attribute as CFString, &value) == .success else { return [] }
        return (value as? [AXUIElement]) ?? []
    }
}
