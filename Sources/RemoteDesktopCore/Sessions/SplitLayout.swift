import Foundation

/// Grid of a split tab: which panes go into which column, left to right, top to bottom.
public enum SplitLayout {
    public static let maxPanes = 4

    /// Pane indices per column: 1 → one column; 2 → side by side; 3 → one left, two stacked right;
    /// 4 → 2×2 (row-major: 0 1 / 2 3). Counts above `maxPanes` are clamped.
    public static func columns(for count: Int) -> [[Int]] {
        switch min(count, maxPanes) {
        case ...0: []
        case 1: [[0]]
        case 2: [[0], [1]]
        case 3: [[0], [1, 2]]
        default: [[0, 2], [1, 3]]
        }
    }
}
