import Testing
@testable import RemoteDesktopCore

@Suite struct TerminalOptionKeysTests {
    @Test func optionArrowsJumpWords() {
        #expect(TerminalOptionKeys.sequence(keyCode: 123, option: true, otherModifiers: false) == "\u{1b}b")
        #expect(TerminalOptionKeys.sequence(keyCode: 124, option: true, otherModifiers: false) == "\u{1b}f")
    }

    @Test func otherKeysAndCombinationsAreLeftToTheTerminal() {
        #expect(TerminalOptionKeys.sequence(keyCode: 123, option: false, otherModifiers: false) == nil)
        #expect(TerminalOptionKeys.sequence(keyCode: 123, option: true, otherModifiers: true) == nil)
        #expect(TerminalOptionKeys.sequence(keyCode: 0, option: true, otherModifiers: false) == nil)
    }
}
