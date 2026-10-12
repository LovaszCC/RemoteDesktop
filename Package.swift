// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "RemoteDesktop",
    platforms: [.macOS(.v15)],
    products: [
        .executable(name: "RemoteDesktop", targets: ["RemoteDesktop"]),
    ],
    dependencies: [
        .package(url: "https://github.com/groue/GRDB.swift.git", from: "7.0.0"),
        .package(url: "https://github.com/migueldeicaza/SwiftTerm.git", exact: "1.20.0"),
    ],
    targets: [
        .target(
            name: "RemoteDesktopCore",
            dependencies: [.product(name: "GRDB", package: "GRDB.swift")]
        ),
        .executableTarget(
            name: "RemoteDesktop",
            dependencies: [
                "RemoteDesktopCore",
                .product(name: "GRDB", package: "GRDB.swift"),
                .product(name: "SwiftTerm", package: "SwiftTerm"),
                "RDPBridge",
            ]
        ),
        // Static FreeRDP + OpenSSL built by scripts/build-freerdp.sh.
        .binaryTarget(name: "FreeRDP", path: "Vendor/FreeRDP.xcframework"),
        .target(
            name: "RDPBridge",
            dependencies: ["FreeRDP"],
            // Textual includes: as Clang modules, CoreFoundation's REFIID clashes with winpr's.
            cSettings: [.unsafeFlags(["-fno-modules", "-Wno-deprecated-declarations", "-Wno-nullability-completeness", "-Wno-macro-redefined"])],
            linkerSettings: [
                .linkedFramework("AppKit"),
                .linkedFramework("AVFoundation"),
                .linkedFramework("CoreMedia"),
                .linkedFramework("Carbon"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("AudioToolbox"),
                .linkedFramework("CoreGraphics"),
                .linkedFramework("CoreServices"),
                .linkedFramework("IOKit"),
                .linkedFramework("Security"),
                .linkedFramework("SystemConfiguration"),
                .linkedLibrary("z"),
                .linkedLibrary("iconv"),
            ]
        ),
        .testTarget(
            name: "RemoteDesktopCoreTests",
            dependencies: ["RemoteDesktopCore"]
        ),
    ]
)
