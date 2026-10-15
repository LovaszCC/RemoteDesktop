import Foundation

/// "Copy next command output": once armed, captures what the remote side prints after the user's next Enter,
/// as plain text (escape sequences stripped, `\r\n` → `\n`), until the prompt seen at arming time comes back.
/// The output is followed all the time (only its last line is kept while idle), so the prompt is the line the
/// stream ended with when the recorder was armed, parsed the same way as the closing one.
/// Fed with the raw pty stream, so it works in whatever state the shell is in (`sudo -i`, `su`, nested `ssh`,
/// RouterOS), and neither line wrapping nor the scrollback limit affect the result.
public struct OutputRecorder: Sendable {
    public enum Phase: Equatable, Sendable {
        case idle
        /// Waiting for the user to press Enter.
        case armed
        case capturing
    }

    public enum Event: Equatable, Sendable {
        case none
        /// The stream ends with the prompt again: the command has most likely finished.
        case atPrompt
        /// The output passed `maxBytes`; the recorder has been reset.
        case tooLarge
    }

    public static let defaultMaxBytes = 10 * 1024 * 1024

    public private(set) var phase: Phase = .idle
    public private(set) var prompt = ""
    public let maxBytes: Int

    private enum Escape: Sendable {
        case none, esc, escIntermediate, csi, osc, oscEsc
    }

    private var escape = Escape.none
    private var pendingCarriageReturn = false
    /// The rest of the echoed command line, up to the first newline after Enter.
    private var skippingEcho = false
    private var finished: [UInt8] = []
    private var current: [UInt8] = []

    public init(maxBytes: Int = OutputRecorder.defaultMaxBytes) {
        self.maxBytes = maxBytes
    }

    /// Takes the line the output currently ends with as the prompt (an empty one never ends the capture by
    /// itself, only a manual stop does).
    public mutating func arm() {
        prompt = String(decoding: current, as: UTF8.self)
        finished.removeAll()
        skippingEcho = false
        phase = .armed
    }

    /// Keyboard / paste input. Returns true when it started the capture (Enter while armed).
    public mutating func userInput(_ data: ArraySlice<UInt8>) -> Bool {
        guard phase == .armed, data.contains(0x0D) || data.contains(0x0A) else { return false }
        phase = .capturing
        skippingEcho = true
        return true
    }

    public mutating func output(_ data: ArraySlice<UInt8>) -> Event {
        guard phase == .capturing else {
            track(data)
            return .none
        }
        for byte in data { consume(byte) }
        if finished.count + current.count > maxBytes {
            reset()
            return .tooLarge
        }
        return isAtPrompt ? .atPrompt : .none
    }

    /// The captured text without the closing prompt and the newline before it. A last line that doesn't end
    /// with the saved prompt is dropped too: after a manual stop it is usually a changed prompt (`cd`).
    public mutating func finish() -> String {
        var bytes = finished
        if let beforePrompt = textBeforePrompt { bytes += beforePrompt }
        if bytes.last == 0x0A { bytes.removeLast() }
        reset()
        return String(decoding: bytes, as: UTF8.self)
    }

    public mutating func cancel() {
        reset()
    }

    // MARK: Parsing

    /// Outside a capture only the last line matters (it becomes the prompt): skip to the last newline.
    private mutating func track(_ data: ArraySlice<UInt8>) {
        var rest = data
        if let newline = data.lastIndex(of: 0x0A) {
            current.removeAll()
            escape = .none
            pendingCarriageReturn = false
            rest = data[(newline + 1)...]
        }
        for byte in rest { consume(byte) }
    }

    private var isAtPrompt: Bool {
        !skippingEcho && !pendingCarriageReturn && textBeforePrompt != nil
    }

    /// The current, unfinished line minus the saved prompt at its end; nil when it doesn't end with the prompt.
    private var textBeforePrompt: [UInt8]? {
        let promptBytes = Array(Self.trimmedRight(prompt).utf8)
        guard !promptBytes.isEmpty else { return nil }
        let line = Array(Self.trimmedRight(String(decoding: current, as: UTF8.self)).utf8)
        guard line.count >= promptBytes.count, line.suffix(promptBytes.count).elementsEqual(promptBytes) else { return nil }
        return Array(line.dropLast(promptBytes.count))
    }

    private mutating func consume(_ byte: UInt8) {
        switch escape {
        case .esc:
            switch byte {
            case UInt8(ascii: "["): escape = .csi
            case UInt8(ascii: "]"): escape = .osc
            case 0x20...0x2F: escape = .escIntermediate
            default: escape = .none
            }
            return
        case .escIntermediate:
            if (0x30...0x7E).contains(byte) { escape = .none }
            return
        case .csi:
            if (0x40...0x7E).contains(byte) { escape = .none }
            return
        case .osc:
            if byte == 0x07 { escape = .none } else if byte == 0x1B { escape = .oscEsc }
            return
        case .oscEsc:
            escape = byte == UInt8(ascii: "\\") ? .none : .osc
            return
        case .none:
            break
        }

        if pendingCarriageReturn, byte != 0x0A {
            // A lone CR returns to the start of the line: what follows overwrites it (redraws, progress bars).
            current.removeAll()
        }
        pendingCarriageReturn = false

        switch byte {
        case 0x1B:
            escape = .esc
        case 0x0D:
            pendingCarriageReturn = true
        case 0x0A:
            if skippingEcho {
                skippingEcho = false
            } else if phase == .capturing {
                finished += current
                finished.append(0x0A)
            }
            current.removeAll()
        case 0x08:
            removeLastCharacter()
        case 0x09, 0x20...0x7E, 0x80...0xFF:
            current.append(byte)
        default:
            break
        }
    }

    private mutating func removeLastCharacter() {
        while let last = current.last, (0x80...0xBF).contains(last) { current.removeLast() }
        if !current.isEmpty { current.removeLast() }
    }

    /// Back to idle. The current line stays: it is the prompt the next `arm()` takes.
    private mutating func reset() {
        phase = .idle
        escape = .none
        pendingCarriageReturn = false
        skippingEcho = false
        finished.removeAll()
    }

    private static func trimmedRight(_ text: String) -> String {
        var text = Substring(text)
        while let last = text.last, last.isWhitespace { text.removeLast() }
        return String(text)
    }
}
