import AppKit
import RDPBridge
import RemoteDesktopCore
import SwiftUI

/// Container that keeps the FreeRDP view filling it and reports size changes to the bridge.
final class RDPContainerView: NSView {
    weak var bridge: RDPSessionBridge?

    override func layout() {
        super.layout()
        bridge?.view.frame = bounds
        bridge?.viewDidResize()
    }
}

struct RDPHostView: NSViewRepresentable {
    let bridge: RDPSessionBridge

    func makeNSView(context: Context) -> RDPContainerView {
        let container = RDPContainerView()
        attach(to: container)
        return container
    }

    func updateNSView(_ container: RDPContainerView, context: Context) {
        if container.bridge !== bridge || bridge.view.superview !== container { attach(to: container) }
    }

    private func attach(to container: RDPContainerView) {
        container.subviews.forEach { $0.removeFromSuperview() }
        container.bridge = bridge
        let view = bridge.view
        view.removeFromSuperview()
        view.frame = container.bounds
        view.autoresizingMask = [.width, .height]
        container.addSubview(view)
        DispatchQueue.main.async { view.window?.makeFirstResponder(view) }
    }
}

/// Active RDP tab: the desktop plus a connecting overlay or an ended bar.
struct RDPSessionView: View {
    @Environment(AppState.self) private var state
    @Environment(ShortcutStore.self) private var shortcuts
    let session: RDPSession

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Button {
                    state.typeClipboardIntoActiveSession()
                } label: {
                    Label(session.isTyping ? "Typing…" : "Type clipboard", systemImage: "keyboard.badge.ellipsis")
                }
                .disabled(session.status != .connected || session.isTyping)
                .help("Types the local clipboard text where the cursor is on the remote machine (⇧⌘V)")
                Spacer()
            }
            .controlSize(.small)
            .padding(.horizontal, 10)
            .padding(.vertical, 4)
            .background(.bar)
            .overlay(alignment: .bottom) { Divider() }
            ZStack {
                Color.black
                if let bridge = session.bridge {
                    RDPHostView(bridge: bridge)
                        .id(ObjectIdentifier(bridge))
                }
                if session.status == .connecting {
                    ProgressView("Connecting: \(session.title)…")
                        .padding(20)
                        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 10))
                }
            }
            if case .ended(_, let message) = session.status {
                HStack(spacing: 10) {
                    Image(systemName: "bolt.horizontal.circle").foregroundStyle(.orange)
                    Group {
                        if message.isEmpty {
                            Text("The connection dropped unexpectedly (the server gave no reason)")
                        } else {
                            Text(message)
                        }
                    }
                    .lineLimit(2)
                    Spacer()
                    Button("Reconnect", systemImage: "arrow.clockwise") { state.reconnect(session) }
                        .keyboardShortcut(shortcuts.keyboardShortcut(for: .reconnect))
                    Button("Close") { state.closeSession(session.id) }
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(.bar)
            }
        }
    }
}
