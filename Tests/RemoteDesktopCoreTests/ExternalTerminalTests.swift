import Foundation
import GRDB
import Testing
@testable import RemoteDesktopCore

@Suite struct ExternalTerminalTests {
    /// Fresh, isolated defaults domain per test.
    private func makeDefaults() -> (UserDefaults, String) {
        let domain = "RemoteDesktopTests.ExternalTerminal.\(UUID().uuidString)"
        return (UserDefaults(suiteName: domain)!, domain)
    }

    @Test func unsetPrefersGhosttyWhenInstalled() {
        let (defaults, domain) = makeDefaults()
        defer { defaults.removePersistentDomain(forName: domain) }
        #expect(ExternalTerminal.current(in: defaults, isInstalled: { _ in true }) == .ghostty)
    }

    @Test func unsetFallsBackToTerminalWithoutGhostty() {
        let (defaults, domain) = makeDefaults()
        defer { defaults.removePersistentDomain(forName: domain) }
        var asked: [String] = []
        let terminal = ExternalTerminal.current(in: defaults, isInstalled: { asked.append($0); return false })
        #expect(terminal == .terminal)
        #expect(asked == ["com.mitchellh.ghostty"])
    }

    @Test func storedChoiceWinsEvenWhenGhosttyIsInstalled() {
        let (defaults, domain) = makeDefaults()
        defer { defaults.removePersistentDomain(forName: domain) }
        ExternalTerminal.iterm2.apply(to: defaults)
        #expect(ExternalTerminal.current(in: defaults, isInstalled: { _ in true }) == .iterm2)
    }

    @Test func applyRoundTripsEveryTerminal() {
        let (defaults, domain) = makeDefaults()
        defer { defaults.removePersistentDomain(forName: domain) }
        for terminal in ExternalTerminal.allCases {
            terminal.apply(to: defaults)
            #expect(defaults.string(forKey: ExternalTerminal.defaultsKey) == terminal.rawValue)
            #expect(ExternalTerminal.current(in: defaults, isInstalled: { _ in false }) == terminal)
        }
    }

    @Test func unknownStoredValueFallsBackToDefault() {
        let (defaults, domain) = makeDefaults()
        defer { defaults.removePersistentDomain(forName: domain) }
        defaults.set("warp", forKey: ExternalTerminal.defaultsKey)
        #expect(ExternalTerminal.current(in: defaults, isInstalled: { _ in false }) == .terminal)
    }

    @Test func bundleIDsAndNames() {
        #expect(ExternalTerminal.ghostty.bundleID == "com.mitchellh.ghostty")
        #expect(ExternalTerminal.iterm2.bundleID == "com.googlecode.iterm2")
        #expect(ExternalTerminal.terminal.bundleID == "com.apple.Terminal")
        #expect(ExternalTerminal.allCases.map(\.displayName) == ["Ghostty", "iTerm2", "Terminal"])
    }
}

@Suite struct TerminalLaunchTests {
    private let script = URL(filePath: "/tmp/rd/launch.sh")

    @Test func ghosttyRunsScriptWithDashE() {
        let launch = ExternalApps.terminalLaunch(.ghostty, launchScript: script)
        #expect(launch.executable == "/usr/bin/open")
        #expect(launch.arguments == ["-n", "-b", "com.mitchellh.ghostty", "--args", "-e", "/tmp/rd/launch.sh"])
    }

    @Test func terminalAppOpensScript() {
        let launch = ExternalApps.terminalLaunch(.terminal, launchScript: script)
        #expect(launch.executable == "/usr/bin/open")
        #expect(launch.arguments == ["-b", "com.apple.Terminal", "/tmp/rd/launch.sh"])
    }

    @Test func iTermUsesAppleScript() {
        let launch = ExternalApps.terminalLaunch(.iterm2, launchScript: script)
        #expect(launch.executable == "/usr/bin/osascript")
        #expect(launch.arguments == ["-e", """
            tell application id "com.googlecode.iterm2"
                activate
                create window with default profile command "'/tmp/rd/launch.sh'"
            end tell
            """])
    }

    @Test func iTermScriptEscapesQuotesAndBackslashes() throws {
        let path = "/tmp/a \"b\" \\c'd/launch.sh"
        let script = ExternalApps.iTermAppleScript(launchScript: URL(filePath: path))
        let line = try #require(script.split(separator: "\n").first { $0.contains("command") })
        #expect(line.hasSuffix(#"command "'/tmp/a \"b\" \\c'\\''d/launch.sh'""#))

        // AppleScript decodes the literal back to the POSIX-quoted path.
        let literal = line.drop { $0 != "\"" }
        let process = Process()
        process.executableURL = URL(filePath: "/usr/bin/osascript")
        process.arguments = ["-e", "return \(literal)"]
        let output = Pipe()
        process.standardOutput = output
        try process.run()
        process.waitUntilExit()
        let decoded = String(decoding: output.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        #expect(decoded.trimmingCharacters(in: .newlines) == SSHLaunchScript.quote(path))
    }
}

@Suite struct ExternalTerminalMigrationTests {
    @Test func ghosttyConnectionsBecomeExternal() throws {
        let queue = try DatabaseQueue()
        try AppDatabase.migrator.migrate(queue, upTo: "v9-port-forwards")
        try queue.write { db in
            for (id, terminal) in [("g", "ghostty"), ("e", "embedded")] {
                try db.execute(sql: """
                    INSERT INTO connection (id, type, name, host, username, domain, isFavorite, notes, createdAt, updatedAt, sshTerminal)
                    VALUES (?, 'ssh', ?, 'h', '', '', 0, '', '2025-01-01 00:00:00', '2025-01-01 00:00:00', ?)
                    """, arguments: [id, id, terminal])
            }
        }
        let repo = ConnectionRepository(database: try AppDatabase(queue))
        #expect(try repo.connection(id: "g")?.sshTerminal == .external)
        #expect(try repo.connection(id: "e")?.sshTerminal == .embedded)
    }

    @Test func displayNames() {
        #expect(SSHTerminal.allCases == [.embedded, .external])
        #expect(!SSHTerminal.external.displayName.isEmpty)
    }
}
