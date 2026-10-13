import Foundation
import Testing
@testable import RemoteDesktopCore

@Suite struct SplitLayoutTests {
    @Test func columnsPerPaneCount() {
        #expect(SplitLayout.columns(for: 0) == [])
        #expect(SplitLayout.columns(for: 1) == [[0]])
        #expect(SplitLayout.columns(for: 2) == [[0], [1]])
        #expect(SplitLayout.columns(for: 3) == [[0], [1, 2]])
        #expect(SplitLayout.columns(for: 4) == [[0, 2], [1, 3]])
    }

    @Test func clampsAboveMaximum() {
        #expect(SplitLayout.maxPanes == 4)
        #expect(SplitLayout.columns(for: 7) == SplitLayout.columns(for: 4))
    }

    @Test(arguments: 1...4)
    func everyPaneAppearsOnce(count: Int) {
        #expect(SplitLayout.columns(for: count).flatMap { $0 }.sorted() == Array(0..<count))
    }
}

@Suite struct BroadcastGroupTests {
    private func group(_ ids: [String], broadcasting: [String] = []) -> BroadcastGroup {
        var group = BroadcastGroup()
        for id in ids { group.add(id) }
        for id in broadcasting { group.setBroadcasting(id, true) }
        return group
    }

    @Test func addsUpToFourAndRefusesDuplicates() {
        var group = BroadcastGroup()
        let added = ["a", "a", "b", "c", "d", "e"].map { group.add($0) }
        #expect(added == [true, false, true, true, true, false])
        #expect(group.isFull)
        #expect(group.paneIds == ["a", "b", "c", "d"])
        #expect(group.broadcasting.isEmpty)
    }

    @Test func removeDropsBroadcastFlag() {
        var group = group(["a", "b"], broadcasting: ["a", "b"])
        group.remove("a")
        #expect(group.paneIds == ["b"])
        #expect(group.broadcasting == ["b"])
        #expect(!group.isFull)
    }

    @Test func ignoresBroadcastForUnknownPane() {
        var group = group(["a"])
        group.setBroadcasting("x", true)
        #expect(group.broadcasting.isEmpty)
    }

    @Test func noTargetsWhenSourceDoesNotBroadcast() {
        let group = group(["a", "b", "c"], broadcasting: ["b", "c"])
        #expect(group.targets(from: "a", running: ["a", "b", "c"]).isEmpty)
    }

    @Test func targetsAreOtherBroadcastingRunningPanesInOrder() {
        // d: syslog pane, not broadcasting; c: exited.
        let group = group(["a", "b", "c", "d"], broadcasting: ["a", "b", "c"])
        #expect(group.targets(from: "b", running: ["a", "b", "d"]) == ["a"])
        #expect(group.targets(from: "a", running: ["a", "b", "c", "d"]) == ["b", "c"])
    }

    @Test func setAllAndIsAllBroadcasting() {
        var group = group(["a", "b"], broadcasting: ["a"])
        #expect(!group.isAllBroadcasting)
        group.setAll(true)
        #expect(group.isAllBroadcasting)
        #expect(group.targets(from: "b", running: ["a", "b"]) == ["a"])
        group.setAll(false)
        #expect(group.broadcasting.isEmpty)
        #expect(!BroadcastGroup().isAllBroadcasting)
    }
}
