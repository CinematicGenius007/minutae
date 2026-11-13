// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeFeatures",
    platforms: [.macOS("14.4")],
    products: [.library(name: "MinutaeFeatures", targets: ["MinutaeFeatures"])],
    dependencies: [
        .package(path: "../MinutaeCore"),
        .package(path: "../MinutaeDesign"),
        .package(path: "../MinutaeLLM"),
    ],
    targets: [
        .target(name: "MinutaeFeatures", dependencies: ["MinutaeCore", "MinutaeDesign", "MinutaeLLM"]),
        .testTarget(name: "MinutaeFeaturesTests", dependencies: ["MinutaeFeatures"]),
    ],
    swiftLanguageModes: [.v6]
)
