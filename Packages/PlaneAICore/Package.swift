// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "PlaneAICore",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "PlaneAICore", targets: ["PlaneAICore"]),
    ],
    targets: [
        .target(name: "PlaneAICore"),
        .testTarget(name: "PlaneAICoreTests", dependencies: ["PlaneAICore"]),
    ]
)
