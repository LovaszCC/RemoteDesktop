import Foundation

/// App commands whose keyboard shortcut can be changed in Settings.
public enum ShortcutCommand: String, CaseIterable, Codable, Sendable {
    case newRDP, newSSH, newWeb, newMikrotik, newFolder, newCredential, importRDM
    case connect, typeClipboard, splitTerminal, broadcastAll, closeTab, nextTab, previousTab, reconnect, lock

    /// Commands of the File menu (the rest belong to the Connection menu / sessions).
    public static let fileMenu: [ShortcutCommand] = [.newRDP, .newSSH, .newWeb, .newMikrotik, .newFolder, .newCredential, .importRDM]
    public static let connectionMenu: [ShortcutCommand] = allCases.filter { !fileMenu.contains($0) }

    public var title: String {
        switch self {
        case .newRDP: ConnectionType.rdp.newTitle
        case .newSSH: ConnectionType.ssh.newTitle
        case .newWeb: ConnectionType.web.newTitle
        case .newMikrotik: ConnectionType.mikrotik.newTitle
        case .newFolder: String(localized: "New folder", bundle: .core)
        case .newCredential: String(localized: "New credential", bundle: .core)
        case .importRDM: String(localized: "Import from RDM…", bundle: .core)
        case .connect: String(localized: "Connect", bundle: .core)
        case .typeClipboard: String(localized: "Type clipboard into RDP session", bundle: .core)
        case .splitTerminal: String(localized: "Split terminal", bundle: .core)
        case .broadcastAll: String(localized: "Broadcast input to all panes", bundle: .core)
        case .closeTab: String(localized: "Close session", bundle: .core)
        case .nextTab: String(localized: "Next tab", bundle: .core)
        case .previousTab: String(localized: "Previous tab", bundle: .core)
        case .reconnect: String(localized: "Reconnect", bundle: .core)
        case .lock: String(localized: "Lock", bundle: .core)
        }
    }

    public var defaultShortcut: KeyShortcut {
        switch self {
        case .newRDP: KeyShortcut("n")
        case .newSSH: KeyShortcut("n", [.command, .shift])
        case .newWeb: KeyShortcut("b", [.command, .shift])
        case .newMikrotik: KeyShortcut("m", [.command, .shift])
        case .newFolder: KeyShortcut("n", [.command, .option])
        case .newCredential: KeyShortcut("n", [.command, .control])
        case .importRDM: KeyShortcut("i")
        case .connect: KeyShortcut("return")
        case .typeClipboard: KeyShortcut("v", [.command, .shift])
        case .splitTerminal: KeyShortcut("d")
        case .broadcastAll: KeyShortcut("i", [.command, .option])
        case .closeTab: KeyShortcut("w")
        case .nextTab: KeyShortcut("tab", .control)
        case .previousTab: KeyShortcut("tab", [.control, .shift])
        case .reconnect: KeyShortcut("r")
        case .lock: KeyShortcut("l", [.command, .control])
        }
    }
}
