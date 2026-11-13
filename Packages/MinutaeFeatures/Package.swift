// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeFeatures",
    platforms: [.macOS("14.4")],
    products: [.library(name: "MinutaeFeatures", targets: ["MinutaeFeatures"])],
    dependencies: [
        .package(path: "../MinutaeCore"),
        .package(path: "../MinutaeDesign"),
        .package(path: "../MinutaeDictation"),
        .package(path: "../MinutaeLibrary"),
        .package(path: "../MinutaeLLM"),
    ],
    targets: [
        .target(
            name: "MinutaeFeatures",
            dependencies: [
                "MinutaeCore", "MinutaeDesign", "MinutaeDictation", "MinutaeLibrary", "MinutaeLLM",
            ]
        ),
        .testTarget(
            name: "MinutaeFeaturesTests",
            dependencies: ["MinutaeFeatures", "MinutaeCore", "MinutaeDictation", "MinutaeLibrary"]
        ),
    ],
    swiftLanguageModes: [.v6]
)
