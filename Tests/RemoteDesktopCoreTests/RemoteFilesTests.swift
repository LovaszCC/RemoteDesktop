import Foundation
import Testing
@testable import RemoteDesktopCore

@Suite struct RemoteListingParseTests {
    private func data(_ parts: [String]) -> Data { Data(parts.joined().utf8) }

    @Test func parsesFindRecordsWithAnyName() throws {
        let body = "d\t4096\t1760000000.5\tdrwxr-xr-x\0sub dir\0"
            + "f\t6\t1760000100.0\t-rw-r--r--\0a\tfile with tab.txt\0"
            + "l\t10\t1760000200.0\tlrwxrwxrwx\0link\0"
            + "f\t1\t1760000300.0\t-rw-------\0ékezet\0"
        let listing = try RemoteListing.parse(data(["/data\n", "RD-FIND\n", body]))
        #expect(listing.path == "/data")
        #expect(listing.entries.map(\.name) == ["sub dir", "a\tfile with tab.txt", "ékezet", "link"])
        let file = try #require(listing.entries.first { $0.name.hasPrefix("a") })
        #expect(file.kind == .file)
        #expect(file.size == 6)
        #expect(file.permissions == "-rw-r--r--")
        #expect(file.modified == Date(timeIntervalSince1970: 1_760_000_100))
        #expect(listing.entries.first { $0.name == "link" }?.kind == .symlink)
    }

    @Test func parsesLsFallback() throws {
        let now = ISO8601DateFormatter().date(from: "2026-10-13T12:00:00Z")!
        let body = """
        total 16
        -rw-r--r--@ 1 501  0   3 Oct 13 11:00 a  file.txt
        lrwxr-xr-x  1 0    0  10 Oct 12 09:05 link -> a  file.txt
        drwxr-xr-x  2 0    0  64 Mar  2  2024 sub dir
        crw-rw-rw-  1 0    0 1,3 Oct 13 10:00 null
        """
        let listing = try RemoteListing.parse(data(["/x\n", "RD-LS\n", body]), now: now)
        #expect(listing.entries.map(\.name) == ["sub dir", "a  file.txt", "link", "null"])
        let file = try #require(listing.entries.first { $0.name == "a  file.txt" })
        #expect(file.permissions == "-rw-r--r--")
        #expect(file.size == 3)
        let calendar = Calendar(identifier: .gregorian)
        #expect(calendar.component(.year, from: try #require(file.modified)) == 2026)
        let dir = try #require(listing.entries.first { $0.name == "sub dir" })
        #expect(calendar.component(.year, from: try #require(dir.modified)) == 2024)
        #expect(listing.entries.first { $0.name == "link" }?.kind == .symlink)
        #expect(listing.entries.first { $0.name == "null" }?.kind == .other)
    }

    @Test func lsDateInTheFutureIsLastYear() throws {
        let now = ISO8601DateFormatter().date(from: "2026-01-05T12:00:00Z")!
        let listing = try RemoteListing.parse(data(["/x\n", "RD-LS\n", "-rw-r--r-- 1 0 0 1 Dec 30 10:00 old\n"]), now: now)
        let modified = try #require(listing.entries.first?.modified)
        #expect(Calendar(identifier: .gregorian).component(.year, from: modified) == 2025)
    }

    @Test func emptyFolderAndGarbage() throws {
        #expect(try RemoteListing.parse(data(["/empty\n", "RD-FIND\n"])).entries.isEmpty)
        #expect(throws: RemoteListingError.unreadable) { try RemoteListing.parse(Data("oops".utf8)) }
        #expect(throws: RemoteListingError.unreadable) { try RemoteListing.parse(data(["/x\n", "WHAT\n"])) }
    }

    @Test func scriptChangesIntoQuotedPath() {
        #expect(RemoteListing.script(for: "/var/it's").hasPrefix("cd -- '/var/it'\\''s' || exit 2\n"))
        #expect(RemoteListing.script(for: "~").hasPrefix("cd || exit 2\n"))
        #expect(RemoteListing.script(for: "~/src").hasPrefix("cd -- \"$HOME\"/'src' || exit 2\n"))
    }
}

@Suite struct RemotePathTests {
    @Test func joinParentName() {
        #expect(RemotePath.join("/a", "b c") == "/a/b c")
        #expect(RemotePath.join("/", "etc") == "/etc")
        #expect(RemotePath.parent("/a/b") == "/a")
        #expect(RemotePath.parent("/a/b/") == "/a")
        #expect(RemotePath.parent("/a") == "/")
        #expect(RemotePath.parent("/") == "/")
        #expect(RemotePath.name("/var/log") == "log")
    }

    @Test func directoryFromTerminalTitle() {
        #expect(RemotePath.directory(fromTerminalTitle: "root@web: /var/log") == "/var/log")
        #expect(RemotePath.directory(fromTerminalTitle: "admin@web:~/src") == "~/src")
        #expect(RemotePath.directory(fromTerminalTitle: "root@pbx-middle: ~") == "~")
        #expect(RemotePath.directory(fromTerminalTitle: "htop") == nil)
        #expect(RemotePath.directory(fromTerminalTitle: "root@web: vim x") == nil)
    }
}

@Suite struct RemoteCommandTests {
    private let command = RemoteCommand(
        connection: Connection(type: .ssh, name: "w", host: "web", port: 2200, username: "deploy"),
        controlPath: "/tmp/rd-abc")

    @Test func sshReusesTheMaster() {
        #expect(command.ssh("ls") == ["/usr/bin/ssh", "-o", "ControlPath=/tmp/rd-abc", "-o", "ControlMaster=no",
                                      "-o", "BatchMode=yes", "-p", "2200", "deploy@web", "--", "ls"])
        let plain = RemoteCommand(connection: Connection(type: .ssh, name: "w", host: "web"), controlPath: "/tmp/x")
        #expect(plain.ssh("ls").suffix(3) == ["web", "--", "ls"])
        #expect(!plain.ssh("ls").contains("-p"))
    }

    @Test func scpUsesBigPAndBracketsIPv6() {
        #expect(command.download(file: "/a b", to: "/tmp/a b") == ["/usr/bin/scp", "-p", "-q", "-o", "ControlPath=/tmp/rd-abc",
                                                                 "-o", "ControlMaster=no", "-o", "BatchMode=yes",
                                                                 "-P", "2200", "deploy@web:/a b", "/tmp/a b"])
        let v6 = RemoteCommand(connection: Connection(type: .ssh, name: "w", host: "fe80::1", username: "u"), controlPath: "/tmp/x")
        #expect(v6.upload(file: "/l", to: "/r").last == "u@[fe80::1]:/r")
    }

    @Test func controlPathFitsSocketLimit() {
        let path = RemoteCommand.makeControlPath()
        #expect(path.hasPrefix("/tmp/rd-"))
        #expect(path.utf8.count < 104)
        #expect(path != RemoteCommand.makeControlPath())
    }

    @Test func masterOptions() {
        let script = SSHLaunchScript(connection: Connection(type: .ssh, name: "w", host: "web", username: "u"),
                                     password: nil, keyPassphrase: nil)
        var master = script
        master.controlPath = "/tmp/rd-abc"
        let args = master.sshArguments
        #expect(args.contains("-M") && args.contains("-N"))
        #expect(args.contains("ControlPath=/tmp/rd-abc"))
        #expect(args.contains("StrictHostKeyChecking=yes"))
        #expect(args.last == "u@web")
        #expect(!script.sshArguments.contains("-M"))
    }
}

@Suite struct TransferPlanTests {
    private let command = RemoteCommand(connection: Connection(type: .ssh, name: "w", host: "web", username: "u"),
                                        controlPath: "/tmp/rd-abc")

    @Test func downloadStreamsTarWithSeparateGunzip() {
        let plan = TransferPlan.make(name: "it's dir", direction: .download, sourceDirectory: "/srv/a b",
                                     destinationDirectory: "/Users/me/Downloads", compress: true, command: command)
        guard case .stream(let source, let sink) = plan else { Issue.record("not a stream"); return }
        #expect(source.count == 2)
        #expect(source[0].last == "tar -C '/srv/a b' -czf - './it'\\''s dir'")
        #expect(source[1] == ["/usr/bin/gzip", "-dc"])
        #expect(sink == [["/usr/bin/tar", "-xf", "-", "-C", "/Users/me/Downloads"]])

        let raw = TransferPlan.make(name: "d", direction: .download, sourceDirectory: "/s", destinationDirectory: "/l",
                                    compress: false, command: command)
        guard case .stream(let rawSource, _) = raw else { Issue.record("not a stream"); return }
        #expect(rawSource.count == 1)
        #expect(rawSource[0].last == "tar -C '/s' -cf - './d'")
    }

    @Test func uploadDropsMacMetadataAndOwner() {
        let plan = TransferPlan.make(name: "site", direction: .upload, sourceDirectory: "/Users/me",
                                     destinationDirectory: "/var/www", compress: true, command: command)
        guard case .stream(let source, let sink) = plan else { Issue.record("not a stream"); return }
        #expect(source == [["/usr/bin/tar", "--no-mac-metadata", "--no-xattrs", "--no-acls", "-cf", "-",
                            "-C", "/Users/me", "./site"]])
        #expect(sink.count == 2)
        #expect(sink[0] == ["/usr/bin/gzip", "-c"])
        #expect(sink[1].last == "tar -C '/var/www' -xzof -")
    }

    @Test func scpWithoutRemoteTar() {
        let down = TransferPlan.make(name: "f.txt", direction: .download, sourceDirectory: "/r", destinationDirectory: "/l",
                                     compress: true, remoteHasTar: false, command: command)
        #expect(down == .copy(command.download(file: "/r/f.txt", to: "/l/f.txt")))
        let up = TransferPlan.make(name: "f.txt", direction: .upload, sourceDirectory: "/l", destinationDirectory: "/r",
                                   compress: true, remoteHasTar: false, command: command)
        #expect(up == .copy(command.upload(file: "/l/f.txt", to: "/r/f.txt")))
    }

    @Test func duOutputAndScripts() {
        let parsed = TransferPlan.parseDuOutput("1234\t/data/x\n")
        #expect(parsed == 1_263_616, "got \(String(describing: parsed))")
        #expect(TransferPlan.parseDuOutput("") == nil)
        #expect(TransferPlan.remoteSizeScript("/a b") == "du -sk -- '/a b' 2>/dev/null")
        #expect(TransferPlan.remoteExistsScript("/x") == "test -e '/x' || test -L '/x'")
    }
}

@Suite struct ConflictPolicyTests {
    @Test func resolve() {
        #expect(ConflictPolicy.ask.resolve(exists: false) == .replace)
        #expect(ConflictPolicy.ask.resolve(exists: true) == nil)
        #expect(ConflictPolicy.replaceAll.resolve(exists: true) == .replace)
        #expect(ConflictPolicy.skipAll.resolve(exists: true) == .skip)
        #expect(ConflictPolicy.remembering(.replace) == .replaceAll)
        #expect(ConflictPolicy.remembering(.skip) == .skipAll)
    }
}
