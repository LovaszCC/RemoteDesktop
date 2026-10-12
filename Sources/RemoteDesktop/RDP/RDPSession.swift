import AppKit
import RDPBridge
import RemoteDesktopCore

/// One embedded RDP session (FreeRDP); recreates its bridge on reconnect.
@MainActor
@Observable
final class RDPSession: NSObject, Identifiable {
    enum Status: Equatable {
        case connecting
        case connected
        case ended(RDPDisconnectReason, message: String)
    }

    let id = UUID().uuidString
    let connectionId: String
    let title: String
    private(set) var status: Status = .connecting
    /// Changes on reconnect so the host view re-attaches the new FreeRDP view.
    private(set) var bridge: RDPSessionBridge?
    /// True while clipboard text is being typed into the session.
    private(set) var isTyping = false

    /// Builds a fresh configuration (re-resolving the connection, decrypting the password).
    @ObservationIgnored private let makeConfiguration: () throws -> RDPConnectionConfiguration
    @ObservationIgnored var onEnd: ((RDPSession, RDPDisconnectReason) -> Void)?

    init(connection: Connection, makeConfiguration: @escaping () throws -> RDPConnectionConfiguration) {
        connectionId = connection.id
        title = connection.name
        self.makeConfiguration = makeConfiguration
    }

    var isRunning: Bool {
        if case .ended = status { return false }
        return true
    }

    func start() throws {
        let bridge = RDPSessionBridge(configuration: try makeConfiguration())
        bridge.delegate = self
        guard bridge.start() else { throw LaunchError.openFailed(String(localized: "Couldn't create the FreeRDP session.")) }
        status = .connecting
        self.bridge = bridge
    }

    func reconnect() throws {
        bridge?.delegate = nil
        bridge?.stop()
        bridge = nil
        try start()
    }

    /// Types `text` into the focused remote control, then gives keyboard focus back to the desktop.
    func type(_ text: String, completion: @escaping @MainActor (Bool) -> Void) {
        guard let bridge, status == .connected, !isTyping else { return completion(false) }
        isTyping = true
        let keystrokes = RDPTyping.keystrokes(for: text).map { NSNumber(value: $0) }
        bridge.view.window?.makeFirstResponder(bridge.view)
        bridge.typeKeystrokes(keystrokes) { [weak self] finished in
            MainActor.assumeIsolated {
                self?.isTyping = false
                completion(finished)
            }
        }
    }

    func stop() {
        bridge?.delegate = nil
        bridge?.stop()
    }

    /// Initial desktop size before Display Control adapts it to the tab.
    static func initialSize() -> (width: UInt32, height: UInt32) {
        let frame = NSApp.mainWindow?.contentLayoutRect.size ?? CGSize(width: 1280, height: 800)
        let width = UInt32(max(800, min(8192, Int(frame.width) - 280))) & ~1
        let height = UInt32(max(600, min(8192, Int(frame.height) - 40)))
        return (width, height)
    }
}

/// The bridge calls every delegate method on the main thread (checked at runtime).
extension RDPSession: @preconcurrency RDPSessionBridgeDelegate {
    func rdpSessionDidConnect(_ session: RDPSessionBridge) {
        status = .connected
    }

    func rdpSession(
        _ session: RDPSessionBridge,
        didDisconnectWithLastError lastError: UInt32,
        errorInfo: UInt32,
        message: String
    ) {
        let reason = RDPDisconnectReason(lastError: lastError, errorInfo: errorInfo)
        status = .ended(reason, message: message)
        onEnd?(self, reason)
    }

    func rdpSession(_ session: RDPSessionBridge, verifyCertificate certificate: RDPCertificateInfo) -> RDPCertificateDecision {
        CertificatePrompt.ask(certificate, for: title)
    }
}

/// Modal certificate confirmation; FreeRDP stores permanently accepted certificates itself.
@MainActor
enum CertificatePrompt {
    static func ask(_ certificate: RDPCertificateInfo, for name: String) -> RDPCertificateDecision {
        let alert = NSAlert()
        alert.alertStyle = certificate.changed || certificate.hostMismatch ? .critical : .warning
        alert.messageText = certificate.changed
            ? String(localized: "The certificate of \(name) has changed!")
            : String(localized: "Unknown certificate: \(name)")
        var lines = [
            String(localized: "Server: \(certificate.host):\(certificate.port)"),
            String(localized: "Name: \(certificate.commonName)"),
            String(localized: "Subject: \(certificate.subject)"),
            String(localized: "Issuer: \(certificate.issuer)"),
            String(localized: "Fingerprint: \(certificate.fingerprint)"),
        ]
        if certificate.hostMismatch { lines.append("\n" + String(localized: "Warning: the certificate is not for this host name.")) }
        if certificate.changed { lines.append("\n" + String(localized: "Warning: you accepted a different certificate for this server before.")) }
        alert.informativeText = lines.joined(separator: "\n")
        alert.addButton(withTitle: String(localized: "Accept"))
        alert.addButton(withTitle: String(localized: "Just this time"))
        alert.addButton(withTitle: String(localized: "Reject"))
        switch alert.runModal() {
        case .alertFirstButtonReturn: return .acceptPermanently
        case .alertSecondButtonReturn: return .acceptOnce
        default: return .reject
        }
    }
}
