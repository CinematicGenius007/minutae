// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeFeatures",
    platforms: [.macOS("14.4")],
    products: [.library(name: "MinutaeFeatures", targets: ["MinutaeFeatures"])],
    dependencies: [
        .package(path: "../MinutaeDesign"),
        .package(path: "../MinutaeDictation"),
        .package(path: "../MinutaeLibrary"),
        .package(path: "../MinutaeMeetings"),
        .package(path: "../MinutaeCore"),
        .package(path: "../MinutaeAudio"),
        .package(path: "../MinutaeLLM"),
    ],
    targets: [
        .target(
            name: "MinutaeFeatures",
            dependencies: [
                "MinutaeCore", "MinutaeDesign", "MinutaeDictation", "MinutaeLibrary",
                "MinutaeLLM", "MinutaeMeetings",
            ]
        ),
        .testTarget(
            name: "MinutaeFeaturesTests",
            dependencies: [
                "MinutaeFeatures", "MinutaeCore", "MinutaeDictation", "MinutaeLibrary",
                "MinutaeMeetings", "MinutaeAudio",
            ]
        ),
    ],
    swiftLanguageModes: [.v6]
)
