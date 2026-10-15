import AppKit
import RDPBridge
import Testing

@Suite struct RDPKeyboardTranslatorTests {
    // Device-dependent modifier bits, as AppKit reports them.
    static let leftOption = NSEvent.ModifierFlags.option.rawValue | 0x20
    static let rightOption = NSEvent.ModifierFlags.option.rawValue | 0x40
    static let leftCommand = NSEvent.ModifierFlags.command.rawValue | 0x08
    static let leftShift = NSEvent.ModifierFlags.shift.rawValue | 0x02
    static let control = NSEvent.ModifierFlags.control.rawValue | 0x01

    // RDP scancodes (0x100 = extended).
    static let tab: UInt32 = 0x0F, keyC: UInt32 = 0x2E, keyE: UInt32 = 0x12, keyV: UInt32 = 0x2F
    static let lShift: UInt32 = 0x2A, lControl: UInt32 = 0x1D, rControl: UInt32 = 0x11D
    static let lMenu: UInt32 = 0x38, rMenu: UInt32 = 0x138, lWin: UInt32 = 0x15B, capsLock: UInt32 = 0x3A

    static func down(_ code: UInt32) -> RDPKeyAction { .scancode(code, down: true) }
    static func up(_ code: UInt32) -> RDPKeyAction { .scancode(code, down: false) }
    static func unicode(_ text: String) -> [RDPKeyAction] { text.utf16.map { RDPKeyAction.unicode($0) } }

    @Test func plainKeysPassThrough() {
        let t = RDPKeyboardTranslator()
        #expect(t.keyDown(8, scancode: Self.keyC, characters: "c") == [Self.down(Self.keyC)])
        #expect(t.keyUp(8, scancode: Self.keyC) == [Self.up(Self.keyC)])
    }

    @Test func leftOptionTypesTheMacCharacter() {
        let t = RDPKeyboardTranslator()
        #expect(t.modifiersChanged(Self.leftOption).isEmpty)
        #expect(t.keyDown(9, scancode: Self.keyV, characters: "@") == Self.unicode("@"))
        #expect(t.keyUp(9, scancode: Self.keyV).isEmpty)
        #expect(t.modifiersChanged(0).isEmpty)
    }

    @Test func leftOptionWithShiftTypesAndSendsShift() {
        let t = RDPKeyboardTranslator()
        #expect(t.modifiersChanged(Self.leftOption | Self.leftShift) == [Self.down(Self.lShift)])
        #expect(t.keyDown(9, scancode: Self.keyV, characters: "‘") == Self.unicode("‘"))
    }

    @Test func surrogatePairsAreSentAsTwoUnits() {
        let t = RDPKeyboardTranslator()
        _ = t.modifiersChanged(Self.leftOption)
        #expect(t.keyDown(9, scancode: Self.keyV, characters: "😀").count == 2)
    }

    @Test func leftOptionWithNonCharacterKeyIsAlt() {
        let t = RDPKeyboardTranslator()
        _ = t.modifiersChanged(Self.leftOption)
        #expect(t.keyDown(48, scancode: Self.tab, characters: "\t") == [Self.down(Self.lMenu), Self.down(Self.tab)])
        #expect(t.keyUp(48, scancode: Self.tab) == [Self.up(Self.tab)])
        #expect(t.modifiersChanged(0) == [Self.up(Self.lMenu)])
    }

    @Test func functionKeyCharactersAreNotTyped() {
        let t = RDPKeyboardTranslator()
        _ = t.modifiersChanged(Self.leftOption)
        let f4: UInt32 = 0x3E
        #expect(t.keyDown(118, scancode: f4, characters: "\u{F707}") == [Self.down(Self.lMenu), Self.down(f4)])
    }

    @Test func rightOptionTypesTheMacCharacterToo() {
        let t = RDPKeyboardTranslator()
        let keyQ: UInt32 = 0x10
        #expect(t.modifiersChanged(Self.rightOption).isEmpty)
        #expect(t.keyDown(12, scancode: keyQ, characters: "@") == Self.unicode("@"))
        #expect(t.keyUp(12, scancode: keyQ).isEmpty)
        #expect(t.modifiersChanged(0).isEmpty)
    }

    @Test func rightOptionWithNonCharacterKeyIsRightAlt() {
        let t = RDPKeyboardTranslator()
        _ = t.modifiersChanged(Self.rightOption)
        #expect(t.keyDown(48, scancode: Self.tab, characters: "\t") == [Self.down(Self.rMenu), Self.down(Self.tab)])
        _ = t.keyUp(48, scancode: Self.tab)
        #expect(t.modifiersChanged(0) == [Self.up(Self.rMenu)])
    }

    @Test func commandKeyIsControlAndReleasedAtOnce() {
        let t = RDPKeyboardTranslator()
        #expect(t.modifiersChanged(Self.leftCommand).isEmpty)
        #expect(t.keyDown(8, scancode: Self.keyC, characters: "c")
            == [Self.down(Self.rControl), Self.down(Self.keyC), Self.up(Self.keyC)])
        // AppKit may still deliver the keyUp; it must not release twice.
        #expect(t.keyUp(8, scancode: Self.keyC).isEmpty)
        #expect(t.keyDown(9, scancode: Self.keyV, characters: "v") == [Self.down(Self.keyV), Self.up(Self.keyV)])
        #expect(t.modifiersChanged(0) == [Self.up(Self.rControl)])
    }

    @Test func commandTappedAloneIsTheWindowsKey() {
        let t = RDPKeyboardTranslator()
        _ = t.modifiersChanged(Self.leftCommand)
        #expect(t.modifiersChanged(0) == [Self.down(Self.lWin), Self.up(Self.lWin)])
    }

    @Test func controlStaysControlAndOptionWithControlIsAlt() {
        let t = RDPKeyboardTranslator()
        #expect(t.modifiersChanged(Self.control) == [Self.down(Self.lControl)])
        _ = t.modifiersChanged(Self.control | Self.leftOption)
        #expect(t.keyDown(14, scancode: Self.keyE, characters: "e") == [Self.down(Self.lMenu), Self.down(Self.keyE)])
    }

    @Test func deadKeyThenLetterTypesTheComposedCharacter() {
        let t = RDPKeyboardTranslator()
        _ = t.modifiersChanged(Self.leftOption)
        #expect(t.keyDown(14, scancode: Self.keyE, characters: "").isEmpty)
        #expect(t.keyUp(14, scancode: Self.keyE).isEmpty)
        _ = t.modifiersChanged(0)
        #expect(t.keyDown(14, scancode: Self.keyE, characters: "é") == Self.unicode("é"))
        #expect(t.keyUp(14, scancode: Self.keyE).isEmpty)
        #expect(t.keyDown(14, scancode: Self.keyE, characters: "e") == [Self.down(Self.keyE)])
    }

    @Test func capsLockTogglesOnEachChange() {
        let t = RDPKeyboardTranslator()
        let caps = NSEvent.ModifierFlags.capsLock.rawValue
        #expect(t.modifiersChanged(caps) == [Self.down(Self.capsLock), Self.up(Self.capsLock)])
        #expect(t.modifiersChanged(0) == [Self.down(Self.capsLock), Self.up(Self.capsLock)])
    }

    @Test func modifiersWithoutDeviceBitsUseTheLeftKey() {
        let t = RDPKeyboardTranslator()
        #expect(t.modifiersChanged(NSEvent.ModifierFlags.shift.rawValue) == [Self.down(Self.lShift)])
        #expect(t.modifiersChanged(NSEvent.ModifierFlags.option.rawValue) == [Self.up(Self.lShift)])
        #expect(t.keyDown(9, scancode: Self.keyV, characters: "@") == Self.unicode("@"))
    }

    @Test func releaseAllLetsGoOfEverythingHeld() {
        let t = RDPKeyboardTranslator()
        _ = t.modifiersChanged(Self.leftShift | Self.rightOption)
        _ = t.keyDown(48, scancode: Self.tab, characters: "\t")
        #expect(t.releaseAll() == [Self.up(Self.tab), Self.up(Self.rMenu), Self.up(Self.lShift)])
        #expect(t.releaseAll().isEmpty)
    }
}
