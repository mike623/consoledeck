// swift-tools-version:6.0
import PackageDescription

let package = Package(
    name: "ConsoleDeck",
    platforms: [.macOS(.v15)],
    targets: [
        .executableTarget(name: "ConsoleDeck"),
        .testTarget(name: "ConsoleDeckTests", dependencies: ["ConsoleDeck"]),
    ]
)
