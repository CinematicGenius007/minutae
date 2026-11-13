// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeSpeech",
    platforms: [.macOS("14.4")],
    products: [.library(name: "MinutaeSpeech", targets: ["MinutaeSpeech"])],
    targets: [
        // Upstream b5454 binary avoids unsafe source flags; Metal is included. SHA-256 verified from release zip.
        .binaryTarget(name: "whisper", url: "https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-b5454-xcframework.zip", checksum: "e57f8c48933000acabc13bb913fbe82805d483a2deff692cc6738836bd92393b"),
        .target(name: "MinutaeSpeech", dependencies: ["whisper"]),
        .testTarget(name: "MinutaeSpeechTests", dependencies: ["MinutaeSpeech"]),
    ],
    swiftLanguageModes: [.v6]
)
