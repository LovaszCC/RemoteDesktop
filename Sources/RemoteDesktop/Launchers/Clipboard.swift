import AppKit

@MainActor
enum Clipboard {
    /// Copies `text` and clears it after `seconds`, unless something else was copied meanwhile.
    static func copy(_ text: String, clearAfter seconds: Double? = 30) {
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)
        // Hint for clipboard managers not to record secrets.
        pasteboard.setData(Data(), forType: NSPasteboard.PasteboardType("org.nspasteboard.ConcealedType"))
        guard let seconds else { return }
        let changeCount = pasteboard.changeCount
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(seconds))
            if pasteboard.changeCount == changeCount { pasteboard.clearContents() }
        }
    }
}
