import AppKit
import Observation
import RemoteDesktopCore

/// Drives the update window: the silent launch check, the manual check from the menu, and the DMG download.
@Observable
@MainActor
final class UpdateModel {
    enum Phase: Equatable {
        case idle
        case checking
        case upToDate(String)
        case available(UpdateRelease)
        case downloading(UpdateRelease)
        /// The downloaded disk image, opened in Finder.
        case opened(URL)
        /// The release, when known, lets the user retry the download instead of the check.
        case failed(String, UpdateRelease?)
    }

    static let releasesPage = URL(string: "https://github.com/LovaszCC/RemoteDesktop/releases/latest")!

    private(set) var phase: Phase = .idle
    /// The silent check found a newer version; the main window opens the update window and clears this.
    var isWindowRequested = false
    private(set) var lastCheck = UpdateSchedule.lastCheck()
    private var task: Task<Void, Never>?

    let currentVersion = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? ""

    /// Always reports back: available, up to date or the error.
    func checkManually() {
        phase = .checking
        check(silent: false)
    }

    /// At launch: only when enabled and due, and it only speaks up when there is a newer version.
    func checkAutomaticallyIfDue() {
        guard UpdateSchedule.isDue(), task == nil else { return }
        check(silent: true)
    }

    private func check(silent: Bool) {
        task?.cancel()
        task = Task {
            defer { if !Task.isCancelled { task = nil } } // a cancelled task has been replaced
            do {
                let result = try await UpdateChecker(currentVersion: currentVersion).check()
                UpdateSchedule.markChecked()
                lastCheck = UpdateSchedule.lastCheck()
                switch result {
                case .available(let release):
                    phase = .available(release)
                    if silent { isWindowRequested = true }
                case .upToDate(let version):
                    if !silent { phase = .upToDate(version) }
                }
            } catch {
                guard !Task.isCancelled, !silent else { return }
                phase = .failed(error.localizedDescription, nil)
            }
        }
    }

    /// Saves the DMG to Downloads, verifies it and opens it.
    func download() {
        let release: UpdateRelease
        switch phase {
        case .available(let found), .failed(_, let found?): release = found
        default: return
        }
        task?.cancel()
        phase = .downloading(release)
        task = Task {
            defer { if !Task.isCancelled { task = nil } } // a cancelled task has been replaced
            do {
                let downloads = try FileManager.default.url(
                    for: .downloadsDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
                let dmg = try await UpdateDownloader().download(release, to: downloads)
                NSWorkspace.shared.open(dmg)
                phase = .opened(dmg)
            } catch {
                guard !Task.isCancelled else { return }
                phase = .failed(error.localizedDescription, release)
            }
        }
    }

    func cancelDownload() {
        guard case .downloading(let release) = phase else { return }
        task?.cancel()
        task = nil
        phase = .available(release)
    }

    func openReleasePage() {
        let page: URL = switch phase {
        case .available(let release), .downloading(let release), .failed(_, let release?): release.pageURL
        default: Self.releasesPage
        }
        NSWorkspace.shared.open(page)
    }
}
