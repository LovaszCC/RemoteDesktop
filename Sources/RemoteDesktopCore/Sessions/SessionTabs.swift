import Foundation

/// Order and selection of the detail area's tabs: the fixed "details" tab plus one tab per
/// terminal session. `active == nil` means the details tab.
public struct SessionTabs: Equatable, Sendable {
    public private(set) var ids: [String] = []
    public private(set) var active: String?
    /// Colors chosen for open tabs, overriding their automatic color (`nil` value: explicitly no color).
    public private(set) var colorOverrides: [String: LabelColor?] = [:]

    public init() {}

    /// Adds a session tab and makes it active.
    public mutating func open(_ id: String) {
        if !ids.contains(id) { ids.append(id) }
        active = id
    }

    /// Removes a tab; closing the active one selects its right neighbour, else the left one,
    /// else the details tab.
    public mutating func close(_ id: String) {
        guard let index = ids.firstIndex(of: id) else { return }
        ids.remove(at: index)
        colorOverrides.removeValue(forKey: id)
        guard active == id else { return }
        active = ids.isEmpty ? nil : ids[min(index, ids.count - 1)]
    }

    /// Puts `new` in place of `old` (same position; stays active if `old` was). Unknown ids are ignored.
    public mutating func replace(_ old: String, with new: String) {
        guard let index = ids.firstIndex(of: old), !ids.contains(new) else { return }
        ids[index] = new
        if active == old { active = new }
        if let color = colorOverrides.removeValue(forKey: old) { colorOverrides[new] = color }
    }

    /// Sets a tab's color (`nil`: no color); `reset` goes back to the automatic one. Unknown ids are ignored.
    public mutating func setColor(_ color: LabelColor?, for id: String) {
        guard ids.contains(id) else { return }
        colorOverrides[id] = .some(color)
    }

    public mutating func resetColor(for id: String) {
        colorOverrides.removeValue(forKey: id)
    }

    /// The tab's color: its override if set, else `automatic` (the entry's color).
    public func color(for id: String, automatic: LabelColor?) -> LabelColor? {
        if let override = colorOverrides[id] { return override }
        return automatic
    }

    /// Selects a session tab, or the details tab with `nil`. Unknown ids are ignored.
    public mutating func select(_ id: String?) {
        guard id == nil || ids.contains(id!) else { return }
        active = id
    }

    /// Cycles through details + sessions, wrapping around.
    public mutating func cycle(forward: Bool) {
        let all: [String?] = [nil] + ids.map(Optional.some)
        let current = all.firstIndex(of: active) ?? 0
        let next = (current + (forward ? 1 : -1) + all.count) % all.count
        active = all[next]
    }

    /// SwiftTerm reports either a decoded exit code (0...255) or, on one code path, the raw
    /// `waitpid` status (exit code in the high byte). Normalises both to the exit code.
    public static func exitCode(fromReported value: Int32?) -> Int32? {
        guard let value, value > 255 else { return value }
        return (value & 0x7f) == 0 ? (value >> 8) & 0xff : value
    }

    /// A clean `exit` closes the tab; anything else keeps it open to show the error.
    public static func closesTab(exitCode: Int32?) -> Bool {
        exitCode == 0
    }
}
