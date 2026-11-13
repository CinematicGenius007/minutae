// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeFeatures",
    platforms: [.macOS("14.4")],
    products: [.library(name: "MinutaeFeatures", targets: ["MinutaeFeatures"])],
    dependencies: [
        .package(path: "../MinutaeDesign"),
        .package(path: "../MinutaeMeetings"),
        .package(path: "../MinutaeCore"),
        .package(path: "../MinutaeAudio"),
        .package(path: "../MinutaeLLM"),
    ],
    targets: [
        .target(
            name: "MinutaeFeatures",
            dependencies: ["MinutaeDesign", "MinutaeMeetings", "MinutaeCore", "MinutaeLLM"]),
        .testTarget(
            name: "MinutaeFeaturesTests",
            dependencies: ["MinutaeFeatures", "MinutaeCore", "MinutaeMeetings", "MinutaeAudio"]),
    ],
    swiftLanguageModes: [.v6]
)
