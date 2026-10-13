import SwiftUI

/// Name for saving or renaming a saved split.
struct SavedSplitNameSheet: View {
    @Environment(AppState.self) private var state
    @Environment(\.dismiss) private var dismiss
    @State var name: String
    let isRename: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(isRename ? "Rename saved split" : "Save split").font(.headline)
            TextField("Name", text: $name)
                .textFieldStyle(.roundedBorder)
                .onSubmit(save)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
                Button(isRename ? "Rename" : "Save", action: save)
                    .keyboardShortcut(.defaultAction)
                    .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(20)
        .frame(width: 340)
    }

    private func save() {
        guard !name.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        state.commitSavedSplitPrompt(name: name)
    }
}
