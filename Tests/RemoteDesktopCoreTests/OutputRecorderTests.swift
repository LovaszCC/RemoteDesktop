import Testing
@testable import RemoteDesktopCore

@Suite struct OutputRecorderTests {
    private static let prompt = "root@radius:~# "

    /// A recorder that has seen `prompt`, been armed and got Enter after `command`'s echo.
    private func capturing(prompt: String = Self.prompt, command: String = "cat radiusd.conf",
                           maxBytes: Int = OutputRecorder.defaultMaxBytes) -> OutputRecorder {
        var recorder = OutputRecorder(maxBytes: maxBytes)
        _ = recorder.output(bytes("last login\r\n" + prompt))
        recorder.arm()
        _ = recorder.output(bytes(command))
        #expect({ recorder.userInput(bytes("\r")) }())
        return recorder
    }

    private func bytes(_ text: String) -> ArraySlice<UInt8> { Array(text.utf8)[...] }

    // MARK: Basic flow

    @Test func capturesOutputWithoutEchoAndPrompt() {
        var recorder = capturing()
        #expect(recorder.phase == .capturing)
        #expect({ recorder.output(bytes("\r\nline 1\r\nline 2\r\n")) }() == .none)
        #expect({ recorder.output(bytes(Self.prompt)) }() == .atPrompt)
        #expect({ recorder.finish() }() == "line 1\nline 2")
        #expect(recorder.phase == .idle)
    }

    @Test func armTakesTheLastOutputLineAsPrompt() {
        var recorder = OutputRecorder()
        _ = recorder.output(bytes("motd\r\nmore\r\n\u{1b}]0;root@radius: ~\u{07}" + Self.prompt))
        recorder.arm()
        #expect(recorder.prompt == Self.prompt)
        #expect(recorder.phase == .armed)
    }

    @Test func echoArrivingInChunksIsSkipped() {
        var recorder = OutputRecorder()
        _ = recorder.output(bytes(Self.prompt))
        recorder.arm()
        #expect({ recorder.userInput(bytes("\r")) }())
        _ = recorder.output(bytes("cat "))
        _ = recorder.output(bytes("file\r"))
        _ = recorder.output(bytes("\nhello\r\n"))
        _ = recorder.output(bytes(Self.prompt))
        #expect({ recorder.finish() }() == "hello")
    }

    @Test func emptyOutputGivesEmptyText() {
        var recorder = capturing(command: "true")
        #expect({ recorder.output(bytes("\r\n" + Self.prompt)) }() == .atPrompt)
        #expect({ recorder.finish() }() == "")
    }

    @Test func outputWithoutTrailingNewlineKeepsTextBeforePrompt() {
        var recorder = capturing(command: "printf abc")
        #expect({ recorder.output(bytes("\r\nabc" + Self.prompt)) }() == .atPrompt)
        #expect({ recorder.finish() }() == "abc")
    }

    // MARK: Arming and input

    @Test func inputBeforeEnterDoesNotStartCapture() {
        var recorder = OutputRecorder()
        _ = recorder.output(bytes(Self.prompt))
        recorder.arm()
        #expect(!{ recorder.userInput(bytes("cat file")) }())
        #expect(recorder.phase == .armed)
        #expect({ recorder.output(bytes("cat file")) }() == .none)
        #expect({ recorder.userInput(bytes("\r")) }())
        #expect(recorder.phase == .capturing)
    }

    @Test func idleRecorderIgnoresEnter() {
        var recorder = OutputRecorder()
        #expect(!{ recorder.userInput(bytes("\r")) }())
        #expect(recorder.phase == .idle)
    }

    @Test func outputWhileArmedIsNotCaptured() {
        var recorder = OutputRecorder()
        _ = recorder.output(bytes(Self.prompt))
        recorder.arm()
        #expect({ recorder.output(bytes("background noise\r\n" + Self.prompt)) }() == .none)
        _ = recorder.userInput(bytes("\r"))
        _ = recorder.output(bytes("\r\nreal\r\n" + Self.prompt))
        #expect({ recorder.finish() }() == "real")
    }

    @Test func cancelResetsToIdle() {
        var recorder = capturing()
        _ = recorder.output(bytes("\r\npartial\r\n"))
        recorder.cancel()
        #expect(recorder.phase == .idle)
        #expect({ recorder.output(bytes("more\r\n")) }() == .none)
    }

    @Test func recorderCanBeArmedAgainAfterFinishing() {
        var recorder = capturing()
        _ = recorder.output(bytes("\r\nfirst\r\n" + Self.prompt))
        #expect({ recorder.finish() }() == "first")
        recorder.arm()
        #expect(recorder.prompt == Self.prompt)
        _ = recorder.userInput(bytes("\r"))
        _ = recorder.output(bytes("\r\nsecond\r\n" + Self.prompt))
        #expect({ recorder.finish() }() == "second")
    }

    // MARK: Escape sequences and control characters

    @Test func colorsAndTitlesAreStripped() {
        var recorder = capturing()
        _ = recorder.output(bytes("\r\n\u{1b}[01;34mdir\u{1b}[0m \u{1b}]0;title\u{07}file\u{1b}]2;other\u{1b}\\\r\n"))
        _ = recorder.output(bytes("\u{1b}[?2004h" + Self.prompt))
        #expect({ recorder.finish() }() == "dir file")
    }

    @Test func escapeSequenceSplitAcrossChunksIsStripped() {
        var recorder = capturing()
        _ = recorder.output(bytes("\r\nred: \u{1b}[3"))
        _ = recorder.output(bytes("1mtext\u{1b}"))
        _ = recorder.output(bytes("[0m\r\n" + Self.prompt))
        #expect({ recorder.finish() }() == "red: text")
    }

    /// Ctrl+L / ⌘K: bash repaints the prompt after cursor home + erase display, without CR or LF.
    @Test func clearScreenStartsANewLine() {
        var recorder = OutputRecorder()
        _ = recorder.output(bytes(Self.prompt))
        _ = recorder.output(bytes("\u{1b}[H\u{1b}[2J" + Self.prompt))
        recorder.arm()
        #expect(recorder.prompt == Self.prompt)
    }

    @Test(arguments: ["\u{1b}[H", "\u{1b}[5;1H", "\u{1b}[1;1f", "\u{1b}[2J", "\u{1b}[2K", "\u{1b}[1K", "\u{1b}[G", "\u{1b}[1G"])
    func cursorJumpOrLineEraseStartsANewLine(sequence: String) {
        var recorder = capturing()
        _ = recorder.output(bytes("\r\nkeep\r\nold" + sequence + "new\r\n" + Self.prompt))
        #expect({ recorder.finish() }() == "keep\nnew")
    }

    @Test func eraseToEndOfLineKeepsTheLine() {
        var recorder = capturing()
        _ = recorder.output(bytes("\r\ntext\u{1b}[K\u{1b}[0K more\r\n" + Self.prompt))
        #expect({ recorder.finish() }() == "text more")
    }

    @Test func charsetSelectionIsStripped() {
        var recorder = capturing()
        _ = recorder.output(bytes("\r\n\u{1b}(Bplain\r\n" + Self.prompt))
        #expect({ recorder.finish() }() == "plain")
    }

    @Test func loneCarriageReturnOverwritesTheLine() {
        var recorder = capturing()
        _ = recorder.output(bytes("\r\n 10%\r 50%\r100%\r\ndone\r\n" + Self.prompt))
        #expect({ recorder.finish() }() == "100%\ndone")
    }

    @Test func carriageReturnSplitFromLineFeedIsANewline() {
        var recorder = capturing()
        _ = recorder.output(bytes("\r\nfirst\r"))
        _ = recorder.output(bytes("\nsecond\r\n" + Self.prompt))
        #expect({ recorder.finish() }() == "first\nsecond")
    }

    @Test func backspaceDeletesOneCharacter() {
        var recorder = capturing()
        _ = recorder.output(bytes("\r\nabcx\u{08}d árvíz\u{08}\r\n" + Self.prompt))
        #expect({ recorder.finish() }() == "abcd árví")
    }

    @Test func tabsAndUnicodeAreKept() {
        var recorder = capturing()
        _ = recorder.output(bytes("\r\nkey\tvalue ő\r\n" + Self.prompt))
        #expect({ recorder.finish() }() == "key\tvalue ő")
    }

    @Test func otherControlBytesAreDropped() {
        var recorder = capturing()
        _ = recorder.output(bytes("\r\na\u{07}b\u{00}c\r\n" + Self.prompt))
        #expect({ recorder.finish() }() == "abc")
    }

    // MARK: Prompt detection

    @Test func promptInsideTheOutputDoesNotEndTheCapture() {
        var recorder = capturing()
        #expect({ recorder.output(bytes("\r\n" + Self.prompt + "echo old\r\nnext")) }() == .none)
        _ = recorder.output(bytes("\r\n" + Self.prompt))
        #expect({ recorder.finish() }() == Self.prompt + "echo old\nnext")
    }

    @Test func promptArrivingInChunksIsDetected() {
        var recorder = capturing()
        _ = recorder.output(bytes("\r\nout\r\n"))
        #expect({ recorder.output(bytes("root@ra")) }() == .none)
        #expect({ recorder.output(bytes("dius:~# ")) }() == .atPrompt)
        #expect({ recorder.finish() }() == "out")
    }

    @Test func trailingSpacesDoNotMatter() {
        var recorder = capturing(prompt: "$   ")
        #expect({ recorder.output(bytes("\r\nout\r\n$")) }() == .atPrompt)
        #expect({ recorder.finish() }() == "out")
    }

    @Test func pendingCarriageReturnIsNotAtPrompt() {
        var recorder = capturing()
        #expect({ recorder.output(bytes("\r\nout\r\n" + Self.prompt + "\r")) }() == .none)
    }

    @Test func mikrotikPromptWithColorsIsDetected() {
        let prompt = "\u{1b}[m\u{1b}[36m[admin@MikroTik] > \u{1b}[m"
        var recorder = OutputRecorder()
        _ = recorder.output(bytes("\r\n\r\n" + prompt))
        recorder.arm()
        #expect(recorder.prompt == "[admin@MikroTik] > ")
        _ = recorder.output(bytes("/export"))
        _ = recorder.userInput(bytes("\r"))
        _ = recorder.output(bytes("\r\n# 2026-10-15 by RouterOS 7.16\r\n\u{1b}[32m/interface bridge\u{1b}[m\r\nadd name=bridge\r\n"))
        #expect({ recorder.output(bytes("\r\r\n" + prompt)) }() == .atPrompt)
        #expect({ recorder.finish() }() == "# 2026-10-15 by RouterOS 7.16\n/interface bridge\nadd name=bridge\n")
    }

    @Test func emptyPromptNeverEndsTheCapture() {
        var recorder = OutputRecorder()
        recorder.arm()
        _ = recorder.userInput(bytes("\r"))
        #expect({ recorder.output(bytes("\r\nout\r\n")) }() == .none)
        #expect({ recorder.finish() }() == "out")
    }

    @Test func manualStopDropsAChangedPrompt() {
        var recorder = capturing(command: "cd /etc && ls")
        #expect({ recorder.output(bytes("\r\nhosts\r\nroot@radius:/etc# ")) }() == .none)
        #expect({ recorder.finish() }() == "hosts")
    }

    // MARK: Size limit

    @Test func largeOutputIsRefused() {
        var recorder = capturing(maxBytes: 10)
        #expect({ recorder.output(bytes("\r\n0123456789")) }() == .none)
        #expect({ recorder.output(bytes("A")) }() == .tooLarge)
        #expect(recorder.phase == .idle)
    }
}
