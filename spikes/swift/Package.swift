// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "MinutaeSpike",
    platforms: [
        .macOS(.v14),
    ],
    products: [
        .executable(name: "MinutaeSpike", targets: ["MinutaeSpike"]),
        .executable(name: "spike-cli", targets: ["SpikeCLI"]),
    ],
    dependencies: [
        .package(url: "https://github.com/argmaxinc/WhisperKit.git", from: "0.13.0"),
    ],
    targets: [
        .executableTarget(
            name: "MinutaeSpike",
            dependencies: [
                .product(name: "WhisperKit", package: "WhisperKit"),
            ],
            path: "Sources/MinutaeSpike",
            exclude: ["Info.plist"],
            linkerSettings: [
                .linkedFramework("AppKit"),
                .linkedFramework("Carbon"),
                .linkedFramework("ApplicationServices"),
                .linkedFramework("AVFoundation"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("AudioToolbox"),
            ]
        ),
        .executableTarget(
            name: "SpikeCLI",
            dependencies: [
                .product(name: "WhisperKit", package: "WhisperKit"),
            ],
            path: "Sources/SpikeCLI",
            linkerSettings: [
                .linkedFramework("AppKit"),
                .linkedFramework("Carbon"),
                .linkedFramework("ApplicationServices"),
                .linkedFramework("AVFoundation"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("AudioToolbox"),
            ]
        ),
    ]
)
