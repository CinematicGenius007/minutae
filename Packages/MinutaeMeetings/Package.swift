// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeMeetings",
    platforms: [.macOS("14.4")],
    products: [.library(name: "MinutaeMeetings", targets: ["MinutaeMeetings"])],
    dependencies: [.package(path: "../MinutaeCore"), .package(path: "../MinutaeAudio")],
    targets: [
        .target(name: "MinutaeMeetings", dependencies: ["MinutaeCore", "MinutaeAudio"]),
        .testTarget(name: "MinutaeMeetingsTests", dependencies: ["MinutaeMeetings"]),
    ],
    swiftLanguageModes: [.v6]
)
