import AppKit
import RemoteDesktopCore
import SwiftTerm
import SwiftUI

/// Hosts a session's long-lived terminal view inside a throwaway container.
struct TerminalHostView: NSViewRepresentable {
    let session: TerminalSession
    /// Whether the terminal takes keyboard focus when shown (only one pane of a split does).
    var takesFocus = true

    func makeNSView(context: Context) -> NSView {
        let container = NSView()
        attach(to: container)
        return container
    }

    func updateNSView(_ container: NSView, context: Context) {
        if session.view.superview !== container { attach(to: container) }
    }

    private func attach(to container: NSView) {
        let terminal = session.view
        terminal.removeFromSuperview()
        terminal.frame = container.bounds
        terminal.autoresizingMask = [.width, .height]
        container.addSubview(terminal)
        if takesFocus { DispatchQueue.main.async { terminal.window?.makeFirstResponder(terminal) } }
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
                    .keyboardShortcut("r")
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

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 4) {
                tab(title: String(localized: "Details"), systemImage: "info.circle", isActive: state.tabs.active == nil, tint: .secondary) {
                    state.selectTab(nil)
                }
                ForEach(state.sessions) { session in
                    tab(
                        title: session.title,
                        systemImage: session.systemImage,
                        isActive: state.tabs.active == session.id,
                        tint: session.tint,
                        close: { state.closeSession(session.id) }
                    ) {
                        state.selectTab(session.id)
                    }
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 5)
        }
        .background(.bar)
        .overlay(alignment: .bottom) { Divider() }
    }

    private func tab(
        title: String,
        systemImage: String,
        isActive: Bool,
        tint: SwiftUI.Color,
        close: (() -> Void)? = nil,
        action: @escaping () -> Void
    ) -> some View {
        HStack(spacing: 6) {
            Image(systemName: systemImage).foregroundStyle(tint)
            Text(title).lineLimit(1).frame(maxWidth: 180, alignment: .leading)
            if let close {
                Button(action: close) {
                    Image(systemName: "xmark").font(.caption2.weight(.bold))
                }
                .buttonStyle(.borderless)
                .help("Close session (⌘W)")
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(isActive ? SwiftUI.Color.accentColor.opacity(0.18) : .clear, in: RoundedRectangle(cornerRadius: 6))
        .contentShape(Rectangle())
        .onTapGesture(perform: action)
    }
}

/// Detail column: tab bar (only while sessions exist) above the details or the active session.
struct DetailArea: View {
    @Environment(AppState.self) private var state

    var body: some View {
        VStack(spacing: 0) {
            if !state.sessions.isEmpty {
                SessionTabBar()
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
