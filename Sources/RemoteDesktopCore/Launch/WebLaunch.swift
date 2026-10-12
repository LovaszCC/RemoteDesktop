import Foundation

/// Helpers for browser connections: URL normalisation and the clipboard login sequence.
public enum WebLaunch {
    /// The URL to open; `https://` is assumed when the stored value has no scheme.
    public static func url(from value: String) -> URL? {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, !trimmed.contains(" ") else { return nil }
        let withScheme = trimmed.contains("://") ? trimmed : "https://" + trimmed
        guard let url = URL(string: withScheme), url.scheme != nil, url.host() != nil else { return nil }
        return url
    }

    /// Values copied to the clipboard one after another: password first, username last,
    /// so the username is on the clipboard and the password one step back in the history.
    public static func clipboardSequence(username: String, password: String?) -> [String] {
        [password ?? "", username].filter { !$0.isEmpty }
    }
}
