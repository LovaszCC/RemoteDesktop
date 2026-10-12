import SwiftUI

struct UnlockView: View {
    @Environment(AppState.self) private var state
    let needsSetup: Bool

    @State private var password = ""
    @State private var confirmation = ""
    @State private var remember = true
    @State private var wrongPassword = false
    @FocusState private var focused: Bool

    var body: some View {
        VStack(spacing: 18) {
            Image(systemName: needsSetup ? "key.fill" : "lock.fill")
                .font(.system(size: 44, weight: .light))
                .foregroundStyle(.secondary)

            VStack(spacing: 4) {
                Text(needsSetup ? "Create master password" : "RemoteDesktop is locked")
                    .font(.title2.weight(.semibold))
                Group {
                    if needsSetup {
                        Text("It encrypts your saved passwords. If you forget it, it can't be recovered.")
                    } else {
                        Text(state.databaseURL.path(percentEncoded: false))
                    }
                }
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .lineLimit(2)
                    .truncationMode(.middle)
            }

            VStack(spacing: 8) {
                SecureField("Master password", text: $password)
                    .focused($focused)
                    .onSubmit(submit)
                if needsSetup {
                    SecureField("Master password again", text: $confirmation)
                        .onSubmit(submit)
                }
                if wrongPassword {
                    Text("Wrong password").font(.caption).foregroundStyle(.red)
                }
            }
            .textFieldStyle(.roundedBorder)
            .frame(width: 280)

            Toggle("Remember on this Mac (Keychain)", isOn: $remember)
                .toggleStyle(.checkbox)

            Button(needsSetup ? "Create" : "Unlock", action: submit)
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
                .disabled(!canSubmit)

            SettingsLink { Text("Other database…") }
                .buttonStyle(.link)
                .font(.callout)
        }
        .padding(40)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .onAppear {
            remember = state.rememberKey
            focused = true
        }
    }

    private var canSubmit: Bool {
        !password.isEmpty && (!needsSetup || password == confirmation)
    }

    private func submit() {
        guard canSubmit else { return }
        if needsSetup {
            state.createVault(password: password, remember: remember)
        } else if !state.unlock(password: password, remember: remember) {
            wrongPassword = true
            password = ""
        }
    }
}
