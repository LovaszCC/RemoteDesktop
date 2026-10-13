import Foundation

/// Local stand-in for an NLA-only RDP server: answers the X.224 request with "HYBRID selected",
/// completes a TLS handshake with a throwaway self-signed certificate, then idles. Enough for FreeRDP
/// to reach its NLA credential check. Uses the system python3 and openssl.
final class FakeNLAServer {
    let port: UInt16
    private let process: Process
    private let directory: URL

    init() throws {
        directory = FileManager.default.temporaryDirectory.appending(path: "fake-nla-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let openssl = Process()
        openssl.executableURL = URL(filePath: "/usr/bin/openssl")
        openssl.arguments = [
            "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1", "-subj", "/CN=fake-nla",
            "-keyout", directory.appending(path: "key.pem").path, "-out", directory.appending(path: "cert.pem").path,
        ]
        openssl.standardOutput = FileHandle.nullDevice
        openssl.standardError = FileHandle.nullDevice
        try openssl.run()
        openssl.waitUntilExit()

        let script = directory.appending(path: "server.py")
        try Self.script.write(to: script, atomically: true, encoding: .utf8)
        let output = Pipe()
        process = Process()
        process.executableURL = URL(filePath: "/usr/bin/python3")
        process.arguments = ["-I", script.path, directory.path]
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        try process.run()
        let line = String(decoding: output.fileHandleForReading.availableData, as: UTF8.self)
        guard let port = UInt16(line.trimmingCharacters(in: .whitespacesAndNewlines)) else {
            process.terminate()
            throw CocoaError(.fileReadCorruptFile)
        }
        self.port = port
    }

    func stop() {
        process.terminate()
        try? FileManager.default.removeItem(at: directory)
    }

    private static let script = """
        import socket, ssl, sys
        d = sys.argv[1]
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(d + "/cert.pem", d + "/key.pem")
        listener = socket.socket()
        listener.bind(("127.0.0.1", 0))
        listener.listen(1)
        print(listener.getsockname()[1], flush=True)
        connection, _ = listener.accept()
        header = connection.recv(4)
        length = (header[2] << 8) | header[3]
        received = len(header)
        while received < length:
            received += len(connection.recv(length - received))
        # X.224 Connection Confirm, RDP_NEG_RSP selecting PROTOCOL_HYBRID (NLA).
        connection.sendall(bytes.fromhex("030000130ed000001234000200080002000000"))
        tls = context.wrap_socket(connection, server_side=True)
        while tls.recv(4096):
            pass
        """
}
