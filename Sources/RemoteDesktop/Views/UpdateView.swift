import AppKit
import RemoteDesktopCore
import SwiftUI

/// Content of the "Software Update" window.
struct UpdateView: View {
    @Environment(UpdateModel.self) private var updates
    @Environment(\.dismissWindow) private var dismissWindow

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            switch updates.phase {
            case .idle:
                Text("Check whether a newer version of RemoteDesktop is available.")
                buttons { Button("Check for Updates") { updates.checkManually() }.keyboardShortcut(.defaultAction) }
            case .checking:
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text("Checking for updates…")
                }
            case .upToDate(let version):
                Text("RemoteDesktop is up to date.").font(.headline)
                Text("You have version \(version).").foregroundStyle(.secondary)
                buttons { Button("OK", action: close).keyboardShortcut(.defaultAction) }
            case .available(let release):
                header(release)
                notes(release.notes)
                buttons {
                    Button("Release page") { updates.openReleasePage() }
                    Spacer()
                    Button("Later", action: close)
                    Button("Download") { updates.download() }.keyboardShortcut(.defaultAction)
                }
            case .downloading(let release):
                header(release)
                ProgressView("Downloading…").progressViewStyle(.linear)
                buttons { Button("Cancel") { updates.cancelDownload() }.keyboardShortcut(.cancelAction) }
            case .opened(let dmg):
                Text("The disk image is open.").font(.headline)
                Text("Quit RemoteDesktop, then drag the new version onto the Applications folder in the disk image window.")
                    .fixedSize(horizontal: false, vertical: true)
                buttons {
                    Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([dmg]) }
                    Button("Done", action: close).keyboardShortcut(.defaultAction)
                }
            case .failed(let message, let release):
                Label(message, systemImage: "exclamationmark.triangle.fill")
                    .fixedSize(horizontal: false, vertical: true)
                buttons {
                    Button("Open release page") { updates.openReleasePage() }
                    Button("Try again") {
                        if release == nil { updates.checkManually() } else { updates.download() }
                    }
                    .keyboardShortcut(.defaultAction)
                }
            }
        }
        .padding(20)
        .frame(width: 460, alignment: .leading)
    }

    private func header(_ release: UpdateRelease) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("RemoteDesktop \(release.version.description) is available.").font(.headline)
            Text("You have version \(updates.currentVersion).").foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private func notes(_ markdown: String) -> some View {
        if !markdown.isEmpty {
            ScrollView {
                Text(Self.render(markdown))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(8)
            }
            .frame(height: 180)
            .background(.background.secondary, in: RoundedRectangle(cornerRadius: 6))
        }
    }

    private func buttons(@ViewBuilder _ content: () -> some View) -> some View {
        HStack {
            Spacer()
            content()
        }
    }

    private func close() { dismissWindow(id: "update") }

    /// GitHub release notes: inline Markdown kept line by line, heading marks dropped.
    static func render(_ markdown: String) -> AttributedString {
        let text = markdown
            .split(separator: "\n", omittingEmptySubsequences: false)
            .map { $0.replacing(/^#+\s*/, with: "") }
            .joined(separator: "\n")
        let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        return (try? AttributedString(markdown: text, options: options)) ?? AttributedString(markdown)
    }
}

/// App-menu item: opens the update window and checks right away.
struct CheckForUpdatesButton: View {
    @Environment(UpdateModel.self) private var updates
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Button("Check for Updates…") {
            updates.checkManually()
            openWindow(id: "update")
        }
    }
}

/// On the main window: runs the silent launch check and opens the update window when it finds a newer version.
struct UpdateWindowOpener: ViewModifier {
    @Environment(UpdateModel.self) private var updates
    @Environment(\.openWindow) private var openWindow

    func body(content: Content) -> some View {
        content
            .task { updates.checkAutomaticallyIfDue() }
            .onChange(of: updates.isWindowRequested) {
                guard updates.isWindowRequested else { return }
                updates.isWindowRequested = false
                openWindow(id: "update")
            }
    }
}
