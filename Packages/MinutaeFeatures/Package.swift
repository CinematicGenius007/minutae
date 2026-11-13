// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeFeatures",
    platforms: [.macOS("14.4")],
    products: [.library(name: "MinutaeFeatures", targets: ["MinutaeFeatures"])],
    dependencies: [.package(path: "../MinutaeDesign")],
    targets: [
        .target(name: "MinutaeFeatures", dependencies: ["MinutaeDesign"]),
        .testTarget(name: "MinutaeFeaturesTests", dependencies: ["MinutaeFeatures"]),
    ],
    swiftLanguageModes: [.v6]
)
