import Foundation

extension Bundle {
    static let coreBundleName = "RemoteDesktop_RemoteDesktopCore"

    /// RemoteDesktopCore's resource bundle (translations). Not `.module`: the accessor older SwiftPM
    /// generates (CI) only probes the .app's root and the build machine's `.build` path, never
    /// `Contents/Resources` where the packaged app keeps it, and calls `fatalError` when both miss.
    /// `.module` remains the fallback for `swift test` and `swift run`.
    nonisolated static let core: Bundle =
        resourceBundle(named: coreBundleName, in: [Bundle.main.resourceURL, Bundle.main.bundleURL]) ?? .module

    /// The first `<name>.bundle` found in `directories`.
    static func resourceBundle(named name: String, in directories: [URL?]) -> Bundle? {
        directories.lazy.compactMap { $0.flatMap { Bundle(url: $0.appending(path: "\(name).bundle")) } }.first
    }
}
