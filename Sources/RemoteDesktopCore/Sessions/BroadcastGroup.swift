import Foundation

/// Panes of a split tab and which of them broadcast input (iTerm style: typing in any broadcasting pane
/// goes to every other broadcasting pane).
public struct BroadcastGroup: Equatable, Sendable {
    public private(set) var paneIds: [String] = []
    public private(set) var broadcasting: Set<String> = []

    public init() {}

    public var isFull: Bool { paneIds.count >= SplitLayout.maxPanes }

    /// Adds a pane (not broadcasting); false when the split is full or the pane is already in it.
    @discardableResult
    public mutating func add(_ id: String) -> Bool {
        guard !isFull, !paneIds.contains(id) else { return false }
        paneIds.append(id)
        return true
    }

    public mutating func remove(_ id: String) {
        paneIds.removeAll { $0 == id }
        broadcasting.remove(id)
    }

    public func isBroadcasting(_ id: String) -> Bool { broadcasting.contains(id) }

    public mutating func setBroadcasting(_ id: String, _ on: Bool) {
        guard paneIds.contains(id) else { return }
        if on { broadcasting.insert(id) } else { broadcasting.remove(id) }
    }

    /// Every pane broadcasts (and there is at least one).
    public var isAllBroadcasting: Bool { !paneIds.isEmpty && broadcasting.count == paneIds.count }

    public mutating func setAll(_ on: Bool) {
        broadcasting = on ? Set(paneIds) : []
    }

    /// Panes that receive input typed into `source`: none unless it broadcasts, else the other broadcasting,
    /// running panes in pane order.
    public func targets(from source: String, running: Set<String>) -> [String] {
        guard broadcasting.contains(source) else { return [] }
        return paneIds.filter { $0 != source && broadcasting.contains($0) && running.contains($0) }
    }
}
