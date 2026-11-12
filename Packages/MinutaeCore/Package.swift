// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeCore",
    platforms: [.macOS("14.4")],
    products: [.library(name: "MinutaeCore", targets: ["MinutaeCore"])],
    targets: [
        .target(name: "MinutaeCore"),
        .testTarget(name: "MinutaeCoreTests", dependencies: ["MinutaeCore"]),
    ],
    swiftLanguageModes: [.v6]
)
