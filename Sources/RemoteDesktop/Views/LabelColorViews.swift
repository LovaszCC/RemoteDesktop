import RemoteDesktopCore
import SwiftUI

extension LabelColor {
    var color: SwiftUI.Color {
        switch self {
        case .red: .red
        case .orange: .orange
        case .yellow: .yellow
        case .green: .green
        case .blue: .blue
        case .purple: .purple
        case .pink: .pink
        case .gray: .gray
        }
    }
}

/// Menu items for a color submenu: the palette plus "None" (or a custom title for the empty choice).
struct LabelColorMenuItems: View {
    let selection: LabelColor?
    var noneTitle: LocalizedStringKey = "None"
    let choose: (LabelColor?) -> Void

    var body: some View {
        Button {
            choose(nil)
        } label: {
            if selection == nil { Label(noneTitle, systemImage: "checkmark") } else { Text(noneTitle) }
        }
        Divider()
        ForEach(LabelColor.allCases, id: \.self) { color in
            Button {
                choose(color)
            } label: {
                Label {
                    Text(verbatim: color.title)
                } icon: {
                    Image(systemName: selection == color ? "checkmark.circle.fill" : "circle.fill")
                        .foregroundStyle(color.color)
                }
            }
        }
    }
}

/// Row of color circles for forms; clicking the selected one again clears it.
struct LabelColorPicker: View {
    @Binding var selection: LabelColor?

    var body: some View {
        HStack(spacing: 6) {
            ForEach(LabelColor.allCases, id: \.self) { color in
                Button {
                    selection = selection == color ? nil : color
                } label: {
                    Circle()
                        .fill(color.color)
                        .frame(width: 16, height: 16)
                        .overlay {
                            if selection == color {
                                Circle().strokeBorder(.primary, lineWidth: 2).padding(-3)
                            }
                        }
                }
                .buttonStyle(.plain)
                .help(Text(verbatim: color.title))
            }
            Button {
                selection = nil
            } label: {
                Image(systemName: "circle.slash")
                    .foregroundStyle(selection == nil ? .primary : .secondary)
            }
            .buttonStyle(.plain)
            .help("No color")
        }
        .padding(.vertical, 2)
    }
}
