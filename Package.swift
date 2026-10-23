// swift-tools-version:6.0
import PackageDescription

let package = Package(
    name: "SthDictate",
    platforms: [.macOS(.v14)],
    targets: [
        .target(name: "DictateCore"),
        .executableTarget(name: "SthDictate", dependencies: ["DictateCore"], swiftSettings: [.swiftLanguageMode(.v5)]),
        .executableTarget(name: "CoreChecks", dependencies: ["DictateCore"], path: "Tests/CoreChecks"),
    ]
)
