import RemoteDesktopCore
import SwiftUI

/// Saved port forwards of an SSH connection, shown in its detail view.
struct PortForwardSection: View {
    @Environment(AppState.self) private var state
    let connection: Connection
    @State private var editing: EditedForward?

    private struct EditedForward: Identifiable {
        let forward: PortForward
        let isNew: Bool
        var id: String { forward.id }
    }

    private var forwards: [PortForward] { state.library.portForwards(for: connection.id) }

    var body: some View {
        @Bindable var state = state
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("Port forwards").font(.headline)
                Spacer()
                Button("New forward", systemImage: "plus") {
                    editing = EditedForward(
                        forward: PortForward(connectionId: connection.id, localPort: 0, remoteHost: "", remotePort: 0),
                        isNew: true)
                }
            }
            if forwards.isEmpty {
                Text("Reach machines behind this SSH server: a port on this Mac is forwarded to a host and port the server can reach.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            ForEach(forwards) { forward in
                PortForwardRow(forward: forward) { editing = EditedForward(forward: forward, isNew: false) }
            }
        }
        .sheet(item: $editing) { edited in
            PortForwardEditor(forward: edited.forward, isNew: edited.isNew)
        }
    }
}

private struct PortForwardRow: View {
    @Environment(AppState.self) private var state
    let forward: PortForward
    let edit: () -> Void

    private var session: PortForwardSession? { state.portForwardSession(forward) }
    private var address: String { "localhost:\(forward.localPort)" }

    var body: some View {
        HStack(alignment: .center, spacing: 10) {
            indicator.frame(width: 16)
            VStack(alignment: .leading, spacing: 2) {
                Text(forward.displayName).fontWeight(.medium)
                Text(verbatim: "\(address) → \(forward.destination)")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
                if case .failed(let message) = session?.status {
                    Text(message)
                        .font(.caption)
                        .foregroundStyle(.red)
                        .textSelection(.enabled)
                }
            }
            Spacer()
            if session?.isRunning == true {
                Button {
                    Clipboard.copy(address, clearAfter: nil)
                    state.showToast(String(localized: "Copied"))
                } label: {
                    Image(systemName: "doc.on.doc")
                }
                .buttonStyle(.borderless)
                .help("Copy address")
                Button("Stop", systemImage: "stop.fill") { state.stopPortForward(forward) }
            } else {
                Button("Start", systemImage: "play.fill") { state.startPortForward(forward) }
            }
        }
        .padding(10)
        .background(.background.secondary, in: RoundedRectangle(cornerRadius: 8))
        .contextMenu {
            Button("Edit…", action: edit)
            Button("Delete", role: .destructive) { state.deletePortForward(forward) }
        }
    }

    @ViewBuilder
    private var indicator: some View {
        switch session?.status {
        case .active:
            Circle().fill(.green).frame(width: 9, height: 9).help("Active")
        case .starting:
            ProgressView().controlSize(.mini).help("Starting…")
        case .failed:
            Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.red).help("Failed")
        case .stopped, nil:
            Circle().strokeBorder(.secondary, lineWidth: 1.5).frame(width: 9, height: 9).help("Stopped")
        }
    }
}

/// Creates or edits a saved port forward.
struct PortForwardEditor: View {
    @Environment(AppState.self) private var state
    @Environment(\.dismiss) private var dismiss
    let original: PortForward
    let isNew: Bool
    @State private var name: String
    @State private var localPort: String
    @State private var remoteHost: String
    @State private var remotePort: String
    @State private var error: String?

    init(forward: PortForward, isNew: Bool) {
        original = forward
        self.isNew = isNew
        _name = State(initialValue: forward.name)
        _localPort = State(initialValue: forward.localPort == 0 ? "" : String(forward.localPort))
        _remoteHost = State(initialValue: forward.remoteHost)
        _remotePort = State(initialValue: forward.remotePort == 0 ? "" : String(forward.remotePort))
    }

    var body: some View {
        VStack(spacing: 0) {
            Form {
                Section(isNew ? LocalizedStringKey("New port forward") : LocalizedStringKey("Edit port forward")) {
                    TextField("Name", text: $name, prompt: Text("Optional"))
                    TextField("Local port", text: $localPort, prompt: Text(verbatim: "13389"))
                    TextField("Remote host", text: $remoteHost, prompt: Text(verbatim: "10.0.0.20"))
                    TextField("Remote port", text: $remotePort, prompt: Text(verbatim: "3389"))
                }
                Text("Connections to localhost on the local port are forwarded through the SSH server to the remote host and port.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if let error {
                    Text(error).foregroundStyle(.red)
                }
            }
            .formStyle(.grouped)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Save") { save(start: false) }
                Button("Save & Start") { save(start: true) }
                    .keyboardShortcut(.defaultAction)
            }
            .padding([.horizontal, .bottom])
        }
        .frame(width: 440)
    }

    private func save(start: Bool) {
        var forward = original
        forward.name = name.trimmingCharacters(in: .whitespaces)
        forward.localPort = Int(localPort.trimmingCharacters(in: .whitespaces)) ?? 0
        forward.remoteHost = remoteHost.trimmingCharacters(in: .whitespaces)
        forward.remotePort = Int(remotePort.trimmingCharacters(in: .whitespaces)) ?? 0
        if let message = state.savePortForward(forward, start: start) {
            error = message
        } else {
            dismiss()
        }
    }
}
