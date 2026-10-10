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
    ],
    targets: [
        .target(
            name: "RemoteDesktopCore",
            dependencies: [.product(name: "GRDB", package: "GRDB.swift")]
        ),
        .executableTarget(
            name: "RemoteDesktop",
            dependencies: ["RemoteDesktopCore", .product(name: "GRDB", package: "GRDB.swift")]
        ),
        .testTarget(
            name: "RemoteDesktopCoreTests",
            dependencies: ["RemoteDesktopCore"]
        ),
    ]
)
