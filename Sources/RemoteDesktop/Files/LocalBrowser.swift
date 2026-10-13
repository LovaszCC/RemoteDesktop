import Foundation

/// The file browser's local side: one folder on this Mac.
@MainActor
@Observable
final class LocalBrowser {
    struct Entry: Identifiable, Hashable {
        var id: URL { url }
        let url: URL
        let name: String
        let isDirectory: Bool
        let size: Int64
        let modified: Date?
        let permissions: String
    }

    private(set) var directory: URL
    private(set) var entries: [Entry] = []
    var listError: String?
    var showsHidden = false {
        didSet { reload() }
    }

    init(directory: URL = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask).first
        ?? FileManager.default.homeDirectoryForCurrentUser) {
        self.directory = directory
        reload()
    }

    func go(to url: URL) {
        directory = url.standardizedFileURL
        reload()
    }

    func goUp() { go(to: directory.deletingLastPathComponent()) }

    func open(_ entry: Entry) {
        if entry.isDirectory { go(to: entry.url) }
    }

    func reload() {
        let keys: [URLResourceKey] = [.isDirectoryKey, .fileSizeKey, .contentModificationDateKey, .isHiddenKey]
        do {
            let urls = try FileManager.default.contentsOfDirectory(
                at: directory, includingPropertiesForKeys: keys, options: showsHidden ? [] : [.skipsHiddenFiles])
            entries = urls.map { url in
                let values = try? url.resourceValues(forKeys: Set(keys))
                let mode = (try? FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as? Int) ?? 0
                let isDirectory = values?.isDirectory ?? false
                return Entry(url: url, name: url.lastPathComponent, isDirectory: isDirectory,
                             size: Int64(values?.fileSize ?? 0), modified: values?.contentModificationDate,
                             permissions: Self.permissionString(mode: mode, isDirectory: isDirectory))
            }
            .sorted { a, b in
                a.isDirectory != b.isDirectory ? a.isDirectory : a.name.localizedStandardCompare(b.name) == .orderedAscending
            }
            listError = nil
        } catch {
            entries = []
            listError = error.localizedDescription
        }
    }

    static func permissionString(mode: Int, isDirectory: Bool) -> String {
        let symbols = Array("rwxrwxrwx")
        let bits = (0..<9).map { index in mode & (1 << (8 - index)) != 0 ? symbols[index] : "-" }
        return (isDirectory ? "d" : "-") + String(bits)
    }

    func exists(_ name: String) -> Bool {
        FileManager.default.fileExists(atPath: directory.appending(path: name).path)
    }
}
