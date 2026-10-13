import Foundation
import Testing
@testable import RemoteDesktopCore

@Suite struct RootDetectionTests {
    @Test func rootUser() {
        #expect(RootDetection.isRootUser("root"))
        #expect(RootDetection.isRootUser(" root "))
        #expect(!RootDetection.isRootUser("Root"))
        #expect(!RootDetection.isRootUser("rootless"))
        #expect(!RootDetection.isRootUser(""))
    }

    @Test func userFromTitle() {
        #expect(RootDetection.user(fromTitle: "root@pbx-middle: ~") == "root")
        #expect(RootDetection.user(fromTitle: "admin@web01.example.com:/var/log") == "admin")
        #expect(RootDetection.user(fromTitle: "rootless@h: ~") == "rootless")
        #expect(RootDetection.user(fromTitle: "vim /etc/hosts") == nil)
        #expect(RootDetection.user(fromTitle: "[admin@MikroTik] >") == nil)
    }

    @Test func titleWinsOverLoginUser() {
        // sudo -i after logging in as a normal user
        #expect(RootDetection.isRoot(loginUser: "admin", terminalTitle: "root@web: ~"))
        // exit back from root
        #expect(!RootDetection.isRoot(loginUser: "admin", terminalTitle: "admin@web: ~"))
        // logged in as root, then su to a normal user
        #expect(!RootDetection.isRoot(loginUser: "root", terminalTitle: "deploy@web: ~"))
    }

    @Test func loginUserWhenTitleSaysNothing() {
        #expect(RootDetection.isRoot(loginUser: "root", terminalTitle: nil))
        #expect(RootDetection.isRoot(loginUser: "root", terminalTitle: "htop"))
        #expect(!RootDetection.isRoot(loginUser: "admin", terminalTitle: nil))
    }
}
