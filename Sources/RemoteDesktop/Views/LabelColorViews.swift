import AppKit
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

    /// Menus draw SF Symbols as monochrome templates (a gray circle), so menu items get a pre-colored
    /// bitmap swatch instead. Drawn on demand, so it follows light / dark mode.
    var menuSwatch: Image {
        let swatchColor = NSColor(color)
        let image = NSImage(size: NSSize(width: 12, height: 12), flipped: false) { rect in
            swatchColor.setFill()
            NSBezierPath(ovalIn: rect.insetBy(dx: 0.5, dy: 0.5)).fill()
            return true
        }
        image.isTemplate = false
        return Image(nsImage: image)
    }
}

/// Menu items for a color submenu: the palette plus "None" (or a custom title for the empty choice).
struct LabelColorMenuItems: View {
    let selection: LabelColor?
    var noneTitle: LocalizedStringKey = "None"
    let choose: (LabelColor?) -> Void

    var body: some View {
        // Toggles: the menu shows its own checkmark next to the current choice.
        Toggle(isOn: Binding(get: { selection == nil }, set: { _ in choose(nil) })) {
            Text(noneTitle)
        }
        Divider()
        ForEach(LabelColor.allCases, id: \.self) { color in
            Toggle(isOn: Binding(get: { selection == color }, set: { _ in choose(color) })) {
                Label {
                    Text(verbatim: color.title)
                } icon: {
                    color.menuSwatch
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
