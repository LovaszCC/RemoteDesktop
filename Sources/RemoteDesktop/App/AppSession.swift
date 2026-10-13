import RemoteDesktopCore

/// An embedded session shown as a tab: SSH terminal, split of SSH terminals or RDP desktop.
@MainActor
enum AppSession: Identifiable {
    case terminal(TerminalSession)
    case split(SplitSession)
    case rdp(RDPSession)

    nonisolated var id: String {
        switch self {
        case .terminal(let session): session.id
        case .split(let session): session.id
        case .rdp(let session): session.id
        }
    }

    /// Connections shown in the tab (every pane of a split).
    var connectionIds: [String] {
        switch self {
        case .terminal(let session): [session.connectionId]
        case .split(let session): session.panes.map(\.connectionId)
        case .rdp(let session): [session.connectionId]
        }
    }

    var title: String {
        switch self {
        case .terminal(let session): session.title
        case .split(let session): session.title
        case .rdp(let session): session.title
        }
    }

    var type: ConnectionType {
        switch self {
        case .terminal(let session): session.type
        case .split: .ssh
        case .rdp: .rdp
        }
    }

    var isRunning: Bool {
        switch self {
        case .terminal(let session): session.isRunning
        case .split(let session): session.isRunning
        case .rdp(let session): session.isRunning
        }
    }

    /// Running SSH processes / RDP connections in the tab (each split pane counts).
    var runningCount: Int {
        switch self {
        case .split(let session): session.panes.filter(\.isRunning).count
        default: isRunning ? 1 : 0
        }
    }

    /// Ends the session without triggering its own exit handling.
    func close() {
        switch self {
        case .terminal(let session):
            session.onExit = nil
            session.terminate()
        case .split(let session):
            session.closeAll()
        case .rdp(let session):
            session.onEnd = nil
            session.stop()
        }
    }
}
