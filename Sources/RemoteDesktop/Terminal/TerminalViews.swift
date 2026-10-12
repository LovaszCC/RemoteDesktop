import AppKit
import RemoteDesktopCore
import SwiftTerm
import SwiftUI

/// Hosts a session's long-lived terminal view inside a throwaway container.
struct TerminalHostView: NSViewRepresentable {
    let session: TerminalSession

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
        DispatchQueue.main.async { terminal.window?.makeFirstResponder(terminal) }
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
            if case .exited(let code) = session.status {
                HStack(spacing: 10) {
                    Image(systemName: "bolt.horizontal.circle").foregroundStyle(.orange)
                    Text(code.map { "Kapcsolat megszakadt (kód: \($0))" } ?? "Kapcsolat megszakadt")
                    Spacer()
                    Button("Újrakapcsolódás", systemImage: "arrow.clockwise") { state.reconnect(session) }
                        .keyboardShortcut("r")
                    Button("Bezárás") { state.closeSession(session.id) }
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(.bar)
            }
        }
    }
}

/// Tab strip above the detail area: "Részletek" followed by one tab per session.
struct SessionTabBar: View {
    @Environment(AppState.self) private var state

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 4) {
                tab(title: "Részletek", systemImage: "info.circle", isActive: state.tabs.active == nil, tint: .secondary) {
                    state.selectTab(nil)
                }
                ForEach(state.sessions) { session in
                    tab(
                        title: session.title,
                        systemImage: session.type.systemImage,
                        isActive: state.tabs.active == session.id,
                        tint: session.isRunning ? session.type.tint : .secondary,
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
                .help("Session bezárása (⌘W)")
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
            case .rdp(let session):
                RDPSessionView(session: session)
            case nil:
                DetailView()
            }
        }
    }
}
