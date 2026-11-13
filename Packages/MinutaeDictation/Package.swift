// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeDictation",
    platforms: [.macOS("14.4")],
    products: [
        .library(name: "MinutaeDictation", targets: ["MinutaeDictation"])
    ],
    dependencies: [
        .package(path: "../MinutaeCore")
    ],
    targets: [
        .target(
            name: "MinutaeDictation",
            dependencies: [
                .product(name: "MinutaeCore", package: "MinutaeCore")
            ]
        ),
        .testTarget(
            name: "MinutaeDictationTests",
            dependencies: [
                "MinutaeDictation",
                .product(name: "MinutaeCore", package: "MinutaeCore"),
            ],
            exclude: ["Fixtures"]
        ),
    ],
    swiftLanguageModes: [.v6]
)
