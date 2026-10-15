import Testing
@testable import RemoteDesktopCore

@Suite struct TerminalClearTests {
    @Test func runningShellIsClearedAndRedrawn() {
        #expect(TerminalClear.action(isAlternateScreen: false, isRunning: true) == .localAndRedraw)
    }

    @Test func exitedSessionIsOnlyClearedLocally() {
        #expect(TerminalClear.action(isAlternateScreen: false, isRunning: false) == .localOnly)
    }

    @Test(arguments: [true, false])
    func fullScreenProgramIsLeftAlone(isRunning: Bool) {
        #expect(TerminalClear.action(isAlternateScreen: true, isRunning: isRunning) == .none)
    }

    @Test func localSequenceErasesScreenAndScrollback() {
        #expect(TerminalClear.localSequence.hasPrefix("\u{1b}[H"))
        #expect(TerminalClear.localSequence.contains("\u{1b}[2J"))
        #expect(TerminalClear.localSequence.contains("\u{1b}[3J"))
    }

    @Test func redrawIsControlL() {
        #expect(TerminalClear.redrawPrompt == [0x0C])
    }
}
