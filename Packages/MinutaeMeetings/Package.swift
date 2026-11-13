// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeMeetings",
    platforms: [.macOS("14.4")],
    products: [.library(name: "MinutaeMeetings", targets: ["MinutaeMeetings"])],
    targets: [
        .target(name: "MinutaeMeetings"),
        .testTarget(name: "MinutaeMeetingsTests", dependencies: ["MinutaeMeetings"]),
    ],
    swiftLanguageModes: [.v6]
)
