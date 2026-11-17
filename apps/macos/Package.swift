// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "LarpCC",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "LarpCC", targets: ["LarpCC"])],
    targets: [
        .target(name: "LarpModel"),
        .executableTarget(name: "LarpCC", dependencies: ["LarpModel"]),
        .testTarget(name: "LarpModelTests", dependencies: ["LarpModel"])
    ]
)
