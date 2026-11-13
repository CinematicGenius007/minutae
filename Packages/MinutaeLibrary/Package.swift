// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeLibrary",
    platforms: [.macOS("14.4")],
    products: [.library(name: "MinutaeLibrary", targets: ["MinutaeLibrary"])],
    dependencies: [.package(path: "../MinutaeCore")],
    targets: [
        .target(
            name: "MinutaeLibrary",
            dependencies: [.product(name: "MinutaeCore", package: "MinutaeCore")]
        ),
        .testTarget(
            name: "MinutaeLibraryTests",
            dependencies: [
                "MinutaeLibrary",
                .product(name: "MinutaeCore", package: "MinutaeCore"),
            ]
        ),
    ],
    swiftLanguageModes: [.v6]
)
