import Foundation

/// POSIX path helpers for the remote side (never `URL`, which would resolve against the local disk).
public enum RemotePath {
    public static func join(_ directory: String, _ name: String) -> String {
        directory.hasSuffix("/") ? directory + name : directory + "/" + name
    }

    public static func parent(_ path: String) -> String {
        var trimmed = path
        while trimmed.count > 1, trimmed.hasSuffix("/") { trimmed.removeLast() }
        guard let slash = trimmed.lastIndex(of: "/") else { return "~" }
        return slash == trimmed.startIndex ? "/" : String(trimmed[..<slash])
    }

    public static func name(_ path: String) -> String {
        path.split(separator: "/").last.map(String.init) ?? path
    }

    /// `cd` to a path: `~` and `~/x` go through `$HOME`, everything else is quoted.
    public static func cdCommand(_ path: String) -> String {
        if path.isEmpty || path == "~" { return "cd" }
        if path.hasPrefix("~/") { return "cd -- \"$HOME\"/" + SSHLaunchScript.quote(String(path.dropFirst(2))) }
        return "cd -- " + SSHLaunchScript.quote(path)
    }

    /// The working directory in a `user@host: dir` terminal title (`/var/log`, `~`, `~/src`), else nil.
    public static func directory(fromTerminalTitle title: String) -> String? {
        guard RootDetection.user(fromTitle: title) != nil, let colon = title.firstIndex(of: ":") else { return nil }
        let path = title[title.index(after: colon)...].trimmingCharacters(in: .whitespaces)
        return path.hasPrefix("/") || path.hasPrefix("~") ? path : nil
    }
}
