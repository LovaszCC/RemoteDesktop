import RemoteDesktopCore

/// An embedded session shown as a tab: SSH terminal or RDP desktop.
@MainActor
enum AppSession: Identifiable {
    case terminal(TerminalSession)
    case rdp(RDPSession)

    nonisolated var id: String {
        switch self {
        case .terminal(let session): session.id
        case .rdp(let session): session.id
        }
    }

    var connectionId: String {
        switch self {
        case .terminal(let session): session.connectionId
        case .rdp(let session): session.connectionId
        }
    }

    var title: String {
        switch self {
        case .terminal(let session): session.title
        case .rdp(let session): session.title
        }
    }

    var type: ConnectionType {
        switch self {
        case .terminal(let session): session.type
        case .rdp: .rdp
        }
    }

    var isRunning: Bool {
        switch self {
        case .terminal(let session): session.isRunning
        case .rdp(let session): session.isRunning
        }
    }

    /// Ends the session without triggering its own exit handling.
    func close() {
        switch self {
        case .terminal(let session):
            session.onExit = nil
            session.terminate()
        case .rdp(let session):
            session.onEnd = nil
            session.stop()
        }
    }
}
