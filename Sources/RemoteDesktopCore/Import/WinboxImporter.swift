import Foundation

public enum WinboxImportError: Error, Equatable {
    case unreadable
}

/// Reads a WinBox address book (`addresses.cdb` / exported `.cdb`): a `0D F0 1D C0` magic, then records of
/// `UInt32 LE length` + `"M2"` + fields. A field is a 3-byte little-endian id and a type byte, followed by its
/// value. WinBox stores the passwords in plain text.
public enum WinboxImporter {
    static let magic: [UInt8] = [0x0D, 0xF0, 0x1D, 0xC0]

    enum FieldID {
        static let address: UInt32 = 0x01
        static let login: UInt32 = 0x02
        static let password: UInt32 = 0x03
        static let note: UInt32 = 0x04
        static let group: UInt32 = 0x08
        static let displayName: UInt32 = 0xFE0009
    }

    /// String fields of every record (non-string fields are read and dropped).
    static func records(_ data: Data) throws -> [[UInt32: String]] {
        let bytes = [UInt8](data)
        guard bytes.count >= 4, Array(bytes[0..<4]) == magic else { throw WinboxImportError.unreadable }
        var records: [[UInt32: String]] = []
        var offset = 4
        while offset < bytes.count {
            guard offset + 4 <= bytes.count else { throw WinboxImportError.unreadable }
            let length = Int(bytes[offset]) | Int(bytes[offset + 1]) << 8 | Int(bytes[offset + 2]) << 16 | Int(bytes[offset + 3]) << 24
            offset += 4
            guard length >= 2, offset + length <= bytes.count,
                  bytes[offset] == UInt8(ascii: "M"), bytes[offset + 1] == UInt8(ascii: "2") else {
                throw WinboxImportError.unreadable
            }
            records.append(try fields(bytes[(offset + 2)..<(offset + length)]))
            offset += length
        }
        return records
    }

    private static func fields(_ record: ArraySlice<UInt8>) throws -> [UInt32: String] {
        var result: [UInt32: String] = [:]
        var index = record.startIndex
        func need(_ count: Int) throws {
            guard index + count <= record.endIndex else { throw WinboxImportError.unreadable }
        }
        while index < record.endIndex {
            try need(4)
            let id = UInt32(record[index]) | UInt32(record[index + 1]) << 8 | UInt32(record[index + 2]) << 16
            let type = record[index + 3]
            index += 4
            switch type {
            case 0x00, 0x01: // bool, no value
                break
            case 0x09: // UInt8
                try need(1)
                index += 1
            case 0x08: // UInt32
                try need(4)
                index += 4
            case 0x21, 0x31: // short string / bytes: UInt8 length
                try need(1)
                let length = Int(record[index])
                index += 1
                try need(length)
                if type == 0x21 { result[id] = String(decoding: record[index..<(index + length)], as: UTF8.self) }
                index += length
            default:
                throw WinboxImportError.unreadable
            }
        }
        return result
    }

    /// MikroTik connections, one per record with an address; the WinBox group becomes the folder.
    public static func parse(_ data: Data) throws -> [ImportedConnection] {
        try records(data).compactMap { fields in
            let address = (fields[FieldID.address] ?? "").trimmingCharacters(in: .whitespaces)
            guard !address.isEmpty else { return nil }
            let (host, winboxPort) = splitAddress(address)
            let note = (fields[FieldID.note] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            let displayName = (fields[FieldID.displayName] ?? "").trimmingCharacters(in: .whitespaces)
            let name = [displayName, note, host].first { !$0.isEmpty } ?? host
            var notes: [String] = []
            if !note.isEmpty, note != name { notes.append(note) }
            if let winboxPort { notes.append(String(localized: "WinBox port \(winboxPort)", bundle: .core)) }
            let group = (fields[FieldID.group] ?? "").trimmingCharacters(in: .whitespaces)
            let password = fields[FieldID.password] ?? ""
            return ImportedConnection(
                type: .mikrotik,
                name: name,
                groupPath: group.isEmpty ? [] : [group],
                host: host,
                port: nil,
                username: (fields[FieldID.login] ?? "").trimmingCharacters(in: .whitespaces),
                domain: "",
                password: password.isEmpty ? nil : password,
                sshKeyPath: nil,
                notes: notes.joined(separator: "\n")
            )
        }
    }

    /// `10.0.0.1:8292` → host and WinBox port; IPv6 (`[fe80::1]:8292`) and bare addresses are kept.
    static func splitAddress(_ address: String) -> (host: String, port: String?) {
        if address.hasPrefix("["), let close = address.firstIndex(of: "]") {
            let host = String(address[address.index(after: address.startIndex)..<close])
            let rest = address[address.index(after: close)...]
            return (host, rest.hasPrefix(":") ? String(rest.dropFirst()) : nil)
        }
        let parts = address.split(separator: ":", omittingEmptySubsequences: false)
        if parts.count == 2, let port = Int(parts[1]), port > 0 { return (String(parts[0]), String(port)) }
        return (address, nil)
    }

    /// Already in the library: a MikroTik connection with the same host and user in that folder.
    public static func isDuplicate(_ item: ImportedConnection, among connections: [Connection], folderId: String?) -> Bool {
        connections.contains {
            $0.type == .mikrotik && $0.folderId == folderId
                && $0.host.caseInsensitiveCompare(item.host) == .orderedSame && $0.username == item.username
        }
    }
}
