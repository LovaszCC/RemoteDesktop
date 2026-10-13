import RemoteDesktopCore
import SwiftUI

/// Split tab: toolbar above a 1–4 pane grid (see `SplitLayout`) with draggable dividers.
struct SplitSessionView: View {
    let split: SplitSession

    var body: some View {
        VStack(spacing: 0) {
            SplitToolbar(split: split)
            HSplitView {
                ForEach(Array(SplitLayout.columns(for: split.panes.count).enumerated()), id: \.offset) { _, column in
                    VSplitView {
                        ForEach(column.map { split.panes[$0] }) { pane in
                            PaneView(split: split, pane: pane)
                        }
                    }
                }
            }
        }
    }
}

/// Add-pane menu and the tab-wide broadcast toggle.
private struct SplitToolbar: View {
    @Environment(AppState.self) private var state
    @Environment(ShortcutStore.self) private var shortcuts
    let split: SplitSession

    var body: some View {
        HStack(spacing: 10) {
            Menu {
                let paneConnectionIds = split.panes.map(\.connectionId).reduce(into: [String]()) { if !$0.contains($1) { $0.append($1) } }
                Section("Again") {
                    ForEach(paneConnectionIds, id: \.self) { id in
                        if let connection = state.library.connections.first(where: { $0.id == id }) {
                            Button(connection.name) { state.addPane(to: split, connectionId: id) }
                        }
                    }
                }
                Section("SSH connections") {
                    ForEach(sshConnections) { connection in
                        Button(connection.name) { state.addPane(to: split, connectionId: connection.id) }
                    }
                }
            } label: {
                Label("Add terminal", systemImage: "plus")
            }
            .fixedSize()
            .disabled(split.isFull)
            .help(split.isFull ? Text("A split view holds at most \(SplitLayout.maxPanes) terminals.") : Text("Add a terminal to the split view"))

            Text(verbatim: "\(split.panes.count)/\(SplitLayout.maxPanes)")
                .font(.caption.monospacedDigit())
                .foregroundStyle(.secondary)

            Spacer()

            Toggle(isOn: Binding(get: { split.isAllBroadcasting }, set: { _ in split.toggleBroadcastAll() })) {
                Label("Broadcast to all panes", systemImage: "antenna.radiowaves.left.and.right")
            }
            .toggleStyle(.button)
            .tint(.orange)
            .help("Type into every pane at once (\(shortcuts.display(.broadcastAll)))")
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(.bar)
        .overlay(alignment: .bottom) { Divider() }
    }

    private var sshConnections: [Connection] {
        state.library.connections
            .filter(\.type.usesSSH)
            .sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
    }
}

/// One terminal of a split: header with title, broadcast toggle and close, then the terminal.
private struct PaneView: View {
    @Environment(AppState.self) private var state
    let split: SplitSession
    let pane: TerminalSession

    private var isBroadcasting: Bool { split.isBroadcasting(pane) }

    private var border: SwiftUI.Color? {
        if split.focusedPaneId == pane.id { return .green }
        return isBroadcasting ? .orange : nil
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 6) {
                Image(systemName: pane.type.systemImage)
                    .foregroundStyle(pane.isRunning ? pane.type.tint : .secondary)
                Text(pane.title)
                    .lineLimit(1)
                    .fontWeight(split.focusedPaneId == pane.id ? .semibold : .regular)
                if isBroadcasting {
                    Text("BROADCAST")
                        .font(.caption2.weight(.bold))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 5)
                        .padding(.vertical, 1)
                        .background(.orange, in: Capsule())
                }
                Spacer(minLength: 4)
                Toggle(isOn: Binding(get: { isBroadcasting }, set: { _ in split.toggleBroadcast(pane) })) {
                    Label("Broadcast", systemImage: "antenna.radiowaves.left.and.right")
                }
                .toggleStyle(.button)
                .labelStyle(.iconOnly)
                .tint(.orange)
                .help("Broadcast input: typing here also goes to the other broadcasting panes")
                Button {
                    state.closePane(pane.id, in: split)
                } label: {
                    Image(systemName: "xmark").font(.caption2.weight(.bold))
                }
                .buttonStyle(.borderless)
                .help("Close this terminal")
            }
            .font(.callout)
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(.bar)

            TerminalHostView(session: pane, takesFocus: split.focusedPaneId == pane.id)
                .id(pane.id)
            SessionExitedBar(session: pane) { state.closePane(pane.id, in: split) }
        }
        .overlay {
            // Green: the pane you type into; orange: other broadcasting panes.
            if let border {
                Rectangle().strokeBorder(border, lineWidth: 2).allowsHitTesting(false)
            }
        }
        .frame(minWidth: 220, minHeight: 140)
    }
}
