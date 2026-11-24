// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "LarpCC",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "LarpCC", targets: ["LarpCC"])],
    targets: [
        .target(name: "LarpModel", resources: [.copy("Resources/cli-v1.schema.json")]),
        .executableTarget(name: "LarpCC", dependencies: ["LarpModel"]),
        .testTarget(name: "LarpModelTests", dependencies: ["LarpModel"], resources: [.copy("Resources/cli-v1.examples.json")]),
        .testTarget(name: "LarpCCTests", dependencies: ["LarpCC", "LarpModel"])
    ]
)
