import Foundation
import RemoteDesktopCore

/// A split tab: 2–4 embedded SSH panes side by side; input typed into a broadcasting pane is mirrored to the
/// other broadcasting panes.
@MainActor
@Observable
final class SplitSession: Identifiable {
    let id = UUID().uuidString
    private(set) var panes: [TerminalSession] = []
    private(set) var group = BroadcastGroup()
    /// Pane that gets keyboard focus when the tab is shown.
    var focusedPaneId: String?
    /// The saved split this tab was opened from or saved as.
    var savedSplitId: String?

    var title: String {
        let names = panes.map(\.title)
        return names.count <= 2 ? names.joined(separator: " + ") : String(localized: "Split (\(names.count))")
    }

    var isRunning: Bool { panes.contains(where: \.isRunning) }
    var isFull: Bool { group.isFull }
    var isBroadcasting: Bool { !group.broadcasting.isEmpty }
    var isAllBroadcasting: Bool { group.isAllBroadcasting }

    func isBroadcasting(_ pane: TerminalSession) -> Bool { group.isBroadcasting(pane.id) }

    /// Adds a pane (not broadcasting); false when the split is already full.
    @discardableResult
    func add(_ pane: TerminalSession) -> Bool {
        guard group.add(pane.id) else { return false }
        panes.append(pane)
        pane.onUserInput = { [weak self] source, data in self?.mirror(data, from: source) }
        pane.onFocus = { [weak self, weak pane] in
            if let id = pane?.id, self?.focusedPaneId != id { self?.focusedPaneId = id }
        }
        focusedPaneId = pane.id
        return true
    }

    /// Removes and terminates a pane.
    func remove(paneId: String) {
        guard let index = panes.firstIndex(where: { $0.id == paneId }) else { return }
        let pane = panes.remove(at: index)
        group.remove(paneId)
        pane.onUserInput = nil
        pane.onFocus = nil
        pane.onExit = nil
        pane.terminate()
        if focusedPaneId == paneId { focusedPaneId = panes[safe: min(index, panes.count - 1)]?.id }
    }

    func toggleBroadcast(_ pane: TerminalSession) {
        group.setBroadcasting(pane.id, !group.isBroadcasting(pane.id))
    }

    func toggleBroadcastAll() {
        group.setAll(!group.isAllBroadcasting)
    }

    func closeAll() {
        for pane in panes {
            pane.onUserInput = nil
            pane.onFocus = nil
            pane.onExit = nil
            pane.terminate()
        }
    }

    private func mirror(_ data: ArraySlice<UInt8>, from source: TerminalSession) {
        let running = Set(panes.filter(\.isRunning).map(\.id))
        let targets = group.targets(from: source.id, running: running)
        for pane in panes where targets.contains(pane.id) {
            pane.write(data)
        }
    }
}

private extension Array {
    subscript(safe index: Int) -> Element? { indices.contains(index) ? self[index] : nil }
}
