// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeSpeech",
    platforms: [.macOS("14.4")],
    products: [.library(name: "MinutaeSpeech", targets: ["MinutaeSpeech"])],
    targets: [
        .target(name: "MinutaeSpeech"),
        .testTarget(name: "MinutaeSpeechTests", dependencies: ["MinutaeSpeech"]),
    ],
    swiftLanguageModes: [.v6]
)
