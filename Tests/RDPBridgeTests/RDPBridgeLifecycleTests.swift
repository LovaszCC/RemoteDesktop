import AppKit
import Foundation
import RDPBridge
import Testing

/// Drives a real FreeRDP session against a closed local port: exercises context creation,
/// settings, the connection thread, the disconnect notification and teardown — no server needed.
@MainActor
@Suite struct RDPBridgeLifecycleTests {
    final class Recorder: NSObject, RDPSessionBridgeDelegate {
        var connected = false
        var ended: (lastError: UInt32, errorInfo: UInt32, message: String)?

        func rdpSessionDidConnect(_ session: RDPSessionBridge) { connected = true }

        func rdpSession(_ session: RDPSessionBridge, didDisconnectWithLastError lastError: UInt32, errorInfo: UInt32, message: String) {
            ended = (lastError, errorInfo, message)
        }

        func rdpSession(_ session: RDPSessionBridge, verifyCertificate certificate: RDPCertificateInfo) -> RDPCertificateDecision {
            .reject
        }
    }

    @Test func versionIsPinned() {
        #expect(RDPSessionBridge.freeRDPVersion().hasPrefix("3.32.1"))
    }

    @Test func failedConnectionReportsDisconnect() async throws {
        let configuration = RDPConnectionConfiguration()
        configuration.host = "127.0.0.1"
        configuration.port = 1 // nothing listens here
        configuration.username = "nobody"
        configuration.domain = ""
        configuration.password = "x"
        configuration.width = 1024
        configuration.height = 768
        configuration.dynamicResolution = true

        let recorder = Recorder()
        let bridge = RDPSessionBridge(configuration: configuration)
        bridge.delegate = recorder
        #expect(bridge.start())

        let deadline = Date().addingTimeInterval(20)
        while recorder.ended == nil, Date() < deadline {
            try await Task.sleep(for: .milliseconds(100))
        }
        let ended = try #require(recorder.ended)
        #expect(!recorder.connected)
        #expect(ended.lastError != 0)
        #expect(!ended.message.isEmpty)
        #expect(!bridge.isConnected)

        bridge.stop()
        try await Task.sleep(for: .milliseconds(500))
    }

    @Test func typingWithoutConnectionFailsFast() async throws {
        let configuration = RDPConnectionConfiguration()
        configuration.host = "127.0.0.1"
        configuration.port = 1
        configuration.username = ""
        configuration.domain = ""
        configuration.width = 800
        configuration.height = 600
        let bridge = RDPSessionBridge(configuration: configuration)

        var result: Bool?
        bridge.typeKeystrokes([0x61, -1]) { result = $0 }
        #expect(result == false) // not started: answered synchronously

        #expect(bridge.start())
        var afterStart: Bool?
        bridge.typeKeystrokes([0x61]) { afterStart = $0 }
        #expect(afterStart == false) // still connecting
        bridge.stop()
        try await Task.sleep(for: .milliseconds(500))
    }
}
