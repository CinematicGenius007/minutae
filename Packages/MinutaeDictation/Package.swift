// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeDictation",
    platforms: [.macOS("14.4")],
    products: [
        .library(name: "MinutaeDictation", targets: ["MinutaeDictation"])
    ],
    dependencies: [
        .package(path: "../MinutaeCore"),
        .package(path: "../MinutaeAudio"),
        .package(path: "../MinutaeSpeech"),
    ],
    targets: [
        .target(
            name: "MinutaeDictation",
            dependencies: [
                .product(name: "MinutaeCore", package: "MinutaeCore"),
                .product(name: "MinutaeAudio", package: "MinutaeAudio"),
                .product(name: "MinutaeSpeech", package: "MinutaeSpeech"),
            ]
        ),
        .testTarget(
            name: "MinutaeDictationTests",
            dependencies: [
                "MinutaeDictation",
                .product(name: "MinutaeCore", package: "MinutaeCore"),
                .product(name: "MinutaeAudio", package: "MinutaeAudio"),
                .product(name: "MinutaeSpeech", package: "MinutaeSpeech"),
            ],
            exclude: ["Fixtures"]
        ),
    ],
    swiftLanguageModes: [.v6]
)
