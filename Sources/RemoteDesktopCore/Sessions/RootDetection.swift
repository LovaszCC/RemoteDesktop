import Foundation

/// Whether an SSH session runs as root: from the login user, or — once the remote shell reports it — from the
/// terminal title most distributions set to `user@host: dir` (so `sudo -i` / `su -` are noticed too).
public enum RootDetection {
    public static func isRootUser(_ username: String) -> Bool {
        username.trimmingCharacters(in: .whitespaces) == "root"
    }

    /// The user of a `user@host…` terminal title, or nil when the title has another form.
    public static func user(fromTitle title: String) -> String? {
        let match = title.trimmingCharacters(in: .whitespaces).prefixMatch(of: /([A-Za-z0-9._-]+)@[A-Za-z0-9._-]+/)
        return match.map { String($0.output.1) }
    }

    /// The title's user when it names one, else the login user.
    public static func isRoot(loginUser: String, terminalTitle: String?) -> Bool {
        if let title = terminalTitle, let user = user(fromTitle: title) { return user == "root" }
        return isRootUser(loginUser)
    }
}
