// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeMeetings",
    platforms: [.macOS("14.4")],
    products: [.library(name: "MinutaeMeetings", targets: ["MinutaeMeetings"])],
    dependencies: [
        .package(path: "../MinutaeCore"),
        .package(path: "../MinutaeAudio"),
        .package(path: "../MinutaeSpeech"),
        .package(path: "../MinutaeLLM")
    ],
    targets: [
        .target(
            name: "MinutaeMeetings",
            dependencies: ["MinutaeCore", "MinutaeAudio", "MinutaeSpeech", "MinutaeLLM"]
        ),
        .testTarget(
            name: "MinutaeMeetingsTests",
            dependencies: ["MinutaeMeetings", "MinutaeLLM"]
        )
    ],
    swiftLanguageModes: [.v6]
)
