// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeDictation",
    platforms: [.macOS("14.4")],
    products: [.library(name: "MinutaeDictation", targets: ["MinutaeDictation"])],
    targets: [
        .target(name: "MinutaeDictation"),
        .testTarget(name: "MinutaeDictationTests", dependencies: ["MinutaeDictation"]),
    ],
    swiftLanguageModes: [.v6]
)
