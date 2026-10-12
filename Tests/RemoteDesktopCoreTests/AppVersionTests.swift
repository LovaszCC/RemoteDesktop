import Testing
@testable import RemoteDesktopCore

@Suite struct AppVersionTests {
    @Test(arguments: [
        ("1.2.3", [1, 2, 3]),
        ("v1.2.3", [1, 2, 3]),
        ("V2.0", [2, 0]),
        (" 1.10.0 ", [1, 10, 0]),
        ("7", [7]),
    ])
    func parses(text: String, components: [Int]) throws {
        #expect(try #require(AppVersion(text)).components == components)
    }

    @Test(arguments: ["", "v", "0.0.0-dev.12", "1..2", "1.2.", "1.-2", "abc", "1.2.3b", "١.٢"])
    func rejects(text: String) {
        #expect(AppVersion(text) == nil)
    }

    @Test func comparesNumerically() throws {
        let v = { (text: String) in try #require(AppVersion(text)) }
        #expect(try v("1.10.0") > v("1.9.0"))
        #expect(try v("1.1.1") > v("1.1.0"))
        #expect(try v("2.0") > v("1.99.99"))
        #expect(try v("1.2") == v("1.2.0"))
        #expect(try !(v("1.2") < v("1.2.0")))
        #expect(try v("1.2.0.1") > v("1.2"))
    }

    @Test func description() throws {
        #expect(try #require(AppVersion("v1.2.0")).description == "1.2.0")
    }
}
