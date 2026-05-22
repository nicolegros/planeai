// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "PlaneAICore",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "PlaneAICore", targets: ["PlaneAICore"]),
    ],
    dependencies: [
        .package(url: "https://github.com/groue/GRDB.swift.git", exact: "7.4.1"),
    ],
    targets: [
        .target(
            name: "PlaneAICore",
            dependencies: [.product(name: "GRDB", package: "GRDB.swift")]
        ),
        .testTarget(name: "PlaneAICoreTests", dependencies: ["PlaneAICore"]),
    ]
)
