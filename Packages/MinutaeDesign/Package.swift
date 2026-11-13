// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeDesign",
    platforms: [.macOS("14.4")],
    products: [.library(name: "MinutaeDesign", targets: ["MinutaeDesign"])],
    targets: [
        .target(name: "MinutaeDesign"),
        .testTarget(name: "MinutaeDesignTests", dependencies: ["MinutaeDesign"]),
    ],
    swiftLanguageModes: [.v6]
)
