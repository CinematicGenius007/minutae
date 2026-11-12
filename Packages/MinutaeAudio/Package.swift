// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeAudio",
    platforms: [.macOS("14.4")],
    products: [.library(name: "MinutaeAudio", targets: ["MinutaeAudio"])],
    targets: [
        .target(name: "MinutaeAudio"),
        .testTarget(name: "MinutaeAudioTests", dependencies: ["MinutaeAudio"]),
    ],
    swiftLanguageModes: [.v6]
)
