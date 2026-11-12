// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeLibrary",
    platforms: [.macOS("14.4")],
    products: [.library(name: "MinutaeLibrary", targets: ["MinutaeLibrary"])],
    targets: [
        .target(name: "MinutaeLibrary"),
        .testTarget(name: "MinutaeLibraryTests", dependencies: ["MinutaeLibrary"]),
    ],
    swiftLanguageModes: [.v6]
)
