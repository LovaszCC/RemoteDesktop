import AppKit
import RemoteDesktopCore
import SwiftTerm
import SwiftUI

/// Hosts a session's long-lived terminal view inside a throwaway container.
struct TerminalHostView: NSViewRepresentable {
    let session: TerminalSession
    /// Whether the terminal takes keyboard focus when shown (only one pane of a split does).
    var takesFocus = true

    func makeNSView(context: Context) -> HostContainer {
        let container = HostContainer()
        attach(to: container)
        return container
    }

    func updateNSView(_ container: HostContainer, context: Context) {
        // Only the newest host may (re)attach the terminal. When a split pane moves to another column, SwiftUI
        // creates a new host and may still update the outgoing one, which would pull the terminal back into a
        // container that is about to leave the window.
        if session.host === container, session.view.superview !== container { attach(to: container) }
    }

    private func attach(to container: HostContainer) {
        let terminal = session.view
        session.host = container
        terminal.removeFromSuperview()
        terminal.frame = container.bounds
        terminal.autoresizingMask = [.width, .height]
        container.addSubview(terminal)
        if takesFocus { DispatchQueue.main.async { terminal.window?.makeFirstResponder(terminal) } }
    }

    /// Keeps the terminal at the container's size, also when it was attached while the container was still 0×0.
    final class HostContainer: NSView {
        override func layout() {
            super.layout()
            for subview in subviews where subview.frame != bounds { subview.frame = bounds }
        }
    }
}

/// Active session's terminal plus the "connection closed" bar when it exited with an error.
struct TerminalSessionView: View {
    @Environment(AppState.self) private var state
    let session: TerminalSession

    var body: some View {
        VStack(spacing: 0) {
            TerminalHostView(session: session)
                .id(session.id)
            SessionExitedBar(session: session) { state.closeSession(session.id) }
        }
    }
}

/// "Connection closed" bar with Reconnect / Close, shown while a terminal has exited.
struct SessionExitedBar: View {
    @Environment(AppState.self) private var state
    @Environment(ShortcutStore.self) private var shortcuts
    let session: TerminalSession
    let close: () -> Void

    var body: some View {
        if case .exited(let code) = session.status {
            HStack(spacing: 10) {
                Image(systemName: "bolt.horizontal.circle").foregroundStyle(.orange)
                if let code {
                    Text("Connection closed (code: \(code))")
                } else {
                    Text("Connection closed")
                }
                Spacer()
                Button("Reconnect", systemImage: "arrow.clockwise") { state.reconnect(session) }
                    .keyboardShortcut(shortcuts.keyboardShortcut(for: .reconnect))
                Button("Close", action: close)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(.bar)
        }
    }
}

/// Tab strip above the detail area: "Details" followed by one tab per session.
struct SessionTabBar: View {
    @Environment(AppState.self) private var state
    @Environment(ShortcutStore.self) private var shortcuts
    /// Tab being renamed inline, and the name typed so far.
    @State private var renaming: String?
    @State private var draftTitle = ""
    @FocusState private var renameFieldFocused: Bool

    var body: some View {
        HStack(spacing: 0) {
            tabs
            if state.canSplitActiveTab {
                Button("Split terminal", systemImage: "rectangle.split.2x1") { state.splitActiveTab() }
                    .labelStyle(.iconOnly)
                    .buttonStyle(.borderless)
                    .help("Open the same server in a new pane next to it (\(shortcuts.display(.splitTerminal)))")
                    .padding(.horizontal, 10)
            }
        }
        .background(.bar)
        .overlay(alignment: .bottom) { Divider() }
    }

    private var tabs: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 4) {
                tab(title: String(localized: "Details"), systemImage: "info.circle", isActive: state.tabs.active == nil, tint: .secondary) {
                    state.selectTab(nil)
                }
                ForEach(state.sessions) { session in
                    let title = state.tabTitle(for: session)
                    tab(
                        title: title,
                        systemImage: session.systemImage,
                        isActive: state.tabs.active == session.id,
                        tint: session.tint,
                        color: state.tabColor(for: session)?.color,
                        renameId: session.id,
                        close: { state.closeSession(session.id) }
                    ) {
                        state.selectTab(session.id)
                    }
                    .help(title == session.title ? Text(verbatim: "") : Text(verbatim: session.title))
                    .contextMenu {
                        Button("Rename…") { startRenaming(session.id, current: title) }
                        Menu("Color") {
                            Button("Entry color") { state.resetTabColor(for: session.id) }
                            LabelColorMenuItems(selection: state.tabColor(for: session)) {
                                state.setTabColor($0, for: session.id)
                            }
                        }
                    }
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 5)
        }
    }

    private func tab(
        title: String,
        systemImage: String,
        isActive: Bool,
        tint: SwiftUI.Color,
        color: SwiftUI.Color? = nil,
        renameId: String? = nil,
        close: (() -> Void)? = nil,
        action: @escaping () -> Void
    ) -> some View {
        HStack(spacing: 6) {
            Image(systemName: systemImage).foregroundStyle(tint)
            if let renameId, renaming == renameId {
                TextField("Tab name", text: $draftTitle)
                    .textFieldStyle(.plain)
                    .frame(width: 160)
                    .focused($renameFieldFocused)
                    .onSubmit { finishRenaming(save: true) }
                    .onExitCommand { finishRenaming(save: false) }
                    .onChange(of: renameFieldFocused) { if !renameFieldFocused { finishRenaming(save: true) } }
            } else {
                Text(title).lineLimit(1).frame(maxWidth: 180, alignment: .leading)
            }
            if let close {
                Button(action: close) {
                    Image(systemName: "xmark").font(.caption2.weight(.bold))
                }
                .buttonStyle(.borderless)
                .help("Close session (\(shortcuts.display(.closeTab)))")
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(tabBackground(isActive: isActive, color: color), in: RoundedRectangle(cornerRadius: 6))
        .overlay(alignment: .bottom) {
            if let color {
                Capsule().fill(color).frame(height: 2).padding(.horizontal, 6)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture(perform: action)
        .simultaneousGesture(TapGesture(count: 2).onEnded {
            if let renameId { startRenaming(renameId, current: title) }
        })
    }

    private func startRenaming(_ id: String, current: String) {
        draftTitle = current
        renaming = id
        DispatchQueue.main.async { renameFieldFocused = true }
    }

    /// Enter or leaving the field saves (empty: back to the automatic name); Esc cancels.
    private func finishRenaming(save: Bool) {
        guard let id = renaming else { return }
        renaming = nil
        if save { state.renameTab(id, to: draftTitle) }
    }
}

private func tabBackground(isActive: Bool, color: SwiftUI.Color?) -> SwiftUI.Color {
    if let color { return color.opacity(isActive ? 0.32 : 0.14) }
    return isActive ? SwiftUI.Color.accentColor.opacity(0.18) : .clear
}

/// Detail column: tab bar (only while sessions exist) above the details or the active session.
struct DetailArea: View {
    @Environment(AppState.self) private var state

    var body: some View {
        VStack(spacing: 0) {
            if !state.sessions.isEmpty {
                SessionTabBar()
            }
            // The tab's color (e.g. red for production) stays visible above the session while typing.
            if let session = state.activeSession, let color = state.tabColor(for: session) {
                Rectangle().fill(color.color).frame(height: 3)
            }
            switch state.activeSession {
            case .terminal(let session):
                TerminalSessionView(session: session)
            case .split(let split):
                SplitSessionView(split: split)
            case .rdp(let session):
                RDPSessionView(session: session)
            case nil:
                DetailView()
            }
        }
    }
}

private extension AppSession {
    var systemImage: String {
        if case .split = self { "rectangle.split.2x2" } else { type.systemImage }
    }

    /// Orange while a split broadcasts input, so it is visible from any tab.
    var tint: SwiftUI.Color {
        if case .split(let split) = self, split.isBroadcasting { return .orange }
        return isRunning ? type.tint : .secondary
    }
}
