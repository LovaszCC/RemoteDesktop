import Foundation
import Testing
@testable import RemoteDesktopCore

/// Real transfers against SSH test servers (e.g. Docker containers with sshd, a `/data` tree and the key in
/// root's authorized_keys). Runs only when `RD_SSH_IT_KEY` (private key) and `RD_SSH_IT_KNOWN_HOSTS` are set;
/// `RD_SSH_IT_PORTS` lists the 127.0.0.1 ports (default 2222: GNU userland, 2223: busybox).
@Suite(.enabled(if: ProcessInfo.processInfo.environment["RD_SSH_IT_KEY"] != nil), .serialized)
struct RemoteFilesIntegrationTests {
    static let environment = ProcessInfo.processInfo.environment
    static let ports = (environment["RD_SSH_IT_PORTS"] ?? "2222,2223").split(separator: ",").compactMap { Int($0) }

    /// A connection master for `port`; stopped by the returned closure.
    private func master(port: Int) async throws -> (RemoteCommand, () -> Void) {
        let key = try #require(Self.environment["RD_SSH_IT_KEY"])
        let knownHosts = try #require(Self.environment["RD_SSH_IT_KNOWN_HOSTS"])
        let controlPath = RemoteCommand.makeControlPath()
        let process = Process()
        process.executableURL = URL(filePath: "/usr/bin/ssh")
        // ssh takes the first value of an option, so these win over the master's StrictHostKeyChecking=yes.
        process.arguments = ["-i", key, "-o", "IdentitiesOnly=yes", "-o", "UserKnownHostsFile=\(knownHosts)",
                             "-o", "StrictHostKeyChecking=accept-new", "-p", String(port)]
            + SSHLaunchScript.masterOptions(controlPath) + ["root@127.0.0.1"]
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
        let command = RemoteCommand(
            connection: Connection(type: .ssh, name: "it", host: "127.0.0.1", port: port, username: "root"),
            controlPath: controlPath)
        for _ in 0..<50 {
            try await Task.sleep(for: .milliseconds(100))
            if try await ProcessRunner.run(command.ssh("true")).succeeded { break }
        }
        return (command, {
            process.terminate()
            try? FileManager.default.removeItem(atPath: controlPath)
        })
    }

    private func scratch() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appending(path: "rd-it-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    private func fileCount(_ url: URL) -> Int {
        FileManager.default.enumerator(at: url, includingPropertiesForKeys: nil)?
            .compactMap { $0 as? URL }.filter { !$0.hasDirectoryPath }.count ?? 0
    }

    @Test(arguments: ports)
    func listsDataFolder(port: Int) async throws {
        let (command, stop) = try await master(port: port)
        defer { stop() }
        let result = try await ProcessRunner.run(command.ssh(RemoteListing.script(for: "/data")))
        #expect(result.succeeded, "\(result.errorOutput)")
        let listing = try RemoteListing.parse(result.output)
        #expect(listing.path == "/data")
        let byName = Dictionary(uniqueKeysWithValues: listing.entries.map { ($0.name, $0) })
        #expect(byName["sub dir"]?.kind == .directory)
        #expect(byName["a file.txt"]?.kind == .file)
        #expect(byName["a file.txt"]?.size == 6)
        #expect(byName["link"]?.kind == .symlink)
        #expect(byName["a file.txt"]?.modified != nil)
        #expect(listing.entries.first?.isDirectory == true)

        let home = try RemoteListing.parse(try await ProcessRunner.run(command.ssh(RemoteListing.script(for: "~"))).output)
        #expect(home.path == "/root")
        let missing = try await ProcessRunner.run(command.ssh(RemoteListing.script(for: "/nope")))
        #expect(!missing.succeeded)
    }

    @Test(arguments: ports, [true, false])
    func downloadsFolderAsOneStream(port: Int, compress: Bool) async throws {
        let (command, stop) = try await master(port: port)
        defer { stop() }
        let local = try scratch()
        defer { try? FileManager.default.removeItem(at: local) }

        let size = TransferPlan.parseDuOutput(String(decoding: try await ProcessRunner.run(
            command.ssh(TransferPlan.remoteSizeScript("/data/sub dir"))).output, as: UTF8.self))
        #expect((size ?? 0) > 0)

        let plan = TransferPlan.make(name: "sub dir", direction: .download, sourceDirectory: "/data",
                                     destinationDirectory: local.path, compress: compress, command: command)
        let transfer = RunningTransfer(plan: plan)
        #expect(await transfer.run() == .succeeded)
        #expect(transfer.bytesCopied > 0)
        let remoteCount = Int(String(decoding: try await ProcessRunner.run(
            command.ssh("find '/data/sub dir' -type f | wc -l")).output, as: UTF8.self)
            .trimmingCharacters(in: .whitespacesAndNewlines))
        #expect(fileCount(local.appending(path: "sub dir")) == remoteCount)
        let sample = try String(contentsOf: local.appending(path: "sub dir/deep/f7.txt"), encoding: .utf8)
        #expect(sample == "line 7\n")
    }

    @Test(arguments: ports, [true, false])
    func uploadsFolderWithoutMacMetadataOrOwner(port: Int, compress: Bool) async throws {
        let (command, stop) = try await master(port: port)
        defer { stop() }
        let local = try scratch()
        defer { try? FileManager.default.removeItem(at: local) }
        let folder = local.appending(path: "up load")
        try FileManager.default.createDirectory(at: folder.appending(path: "inner"), withIntermediateDirectories: true)
        for index in 1...20 { try "n\(index)".write(to: folder.appending(path: "inner/it's \(index).txt"), atomically: true, encoding: .utf8) }
        let target = "/tmp/rd-it-\(UUID().uuidString.prefix(6))"
        _ = try await ProcessRunner.run(command.ssh("mkdir -p \(target)"))
        defer { Task { _ = try? await ProcessRunner.run(command.ssh("rm -rf \(target)")) } }

        let plan = TransferPlan.make(name: "up load", direction: .upload, sourceDirectory: local.path,
                                     destinationDirectory: target, compress: compress, command: command)
        #expect(await RunningTransfer(plan: plan).run() == .succeeded)
        let check = try await ProcessRunner.run(command.ssh(
            "find \(target) -type f | wc -l; find \(target) -name '._*' | wc -l; stat -c %u '\(target)/up load/inner'"))
        let lines = String(decoding: check.output, as: UTF8.self).split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }
        #expect(lines == ["20", "0", "0"])
    }

    @Test(arguments: ports)
    func singleFileBothWays(port: Int) async throws {
        let (command, stop) = try await master(port: port)
        defer { stop() }
        let local = try scratch()
        defer { try? FileManager.default.removeItem(at: local) }

        let down = TransferPlan.make(name: "a file.txt", direction: .download, sourceDirectory: "/data",
                                     destinationDirectory: local.path, compress: true, command: command)
        #expect(await RunningTransfer(plan: down).run() == .succeeded)
        #expect(try String(contentsOf: local.appending(path: "a file.txt"), encoding: .utf8) == "hello\n")

        let target = "/tmp/rd-it-\(UUID().uuidString.prefix(6))"
        _ = try await ProcessRunner.run(command.ssh("mkdir -p \(target)"))
        let up = TransferPlan.make(name: "a file.txt", direction: .upload, sourceDirectory: local.path,
                                   destinationDirectory: target, compress: false, command: command)
        #expect(await RunningTransfer(plan: up).run() == .succeeded)
        let copied = try await ProcessRunner.run(command.ssh("cat '\(target)/a file.txt'; rm -rf \(target)"))
        #expect(String(decoding: copied.output, as: UTF8.self) == "hello\n")

        #expect(try await ProcessRunner.run(command.ssh(TransferPlan.remoteExistsScript("/data/link"))).succeeded)
        #expect(!(try await ProcessRunner.run(command.ssh(TransferPlan.remoteExistsScript("/data/none"))).succeeded))
        let tar = try await ProcessRunner.run(command.ssh(TransferPlan.remoteTarCheckScript))
        #expect(String(decoding: tar.output, as: UTF8.self).contains("yes"))
    }

    @Test(arguments: ports)
    func scpFallbackCopiesAFile(port: Int) async throws {
        let (command, stop) = try await master(port: port)
        defer { stop() }
        let local = try scratch()
        defer { try? FileManager.default.removeItem(at: local) }
        let plan = TransferPlan.make(name: "a file.txt", direction: .download, sourceDirectory: "/data",
                                     destinationDirectory: local.path, compress: true, remoteHasTar: false, command: command)
        let outcome = await RunningTransfer(plan: plan).run()
        // scp needs the server's sftp subsystem; the busybox image may not ship one.
        if outcome == .succeeded {
            #expect(try String(contentsOf: local.appending(path: "a file.txt"), encoding: .utf8) == "hello\n")
        } else {
            #expect(port != 2222, "scp failed on the GNU server: \(outcome)")
        }
    }

    @Test(arguments: ports)
    func cancelStopsAStream(port: Int) async throws {
        let (command, stop) = try await master(port: port)
        defer { stop() }
        let local = try scratch()
        defer { try? FileManager.default.removeItem(at: local) }
        _ = try await ProcessRunner.run(command.ssh("mkdir -p /tmp/rd-big && head -c 200000000 /dev/urandom > /tmp/rd-big/blob"))
        defer { Task { _ = try? await ProcessRunner.run(command.ssh("rm -rf /tmp/rd-big")) } }

        let plan = TransferPlan.make(name: "rd-big", direction: .download, sourceDirectory: "/tmp",
                                     destinationDirectory: local.path, compress: false, command: command)
        let transfer = RunningTransfer(plan: plan)
        async let outcome = transfer.run()
        try await Task.sleep(for: .milliseconds(300))
        transfer.cancel()
        #expect(await outcome == .cancelled)
    }
}
