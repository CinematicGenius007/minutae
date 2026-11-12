// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeLLM",
    platforms: [.macOS("14.4")],
    products: [.library(name: "MinutaeLLM", targets: ["MinutaeLLM"])],
    targets: [
        .target(name: "MinutaeLLM"),
        .testTarget(name: "MinutaeLLMTests", dependencies: ["MinutaeLLM"]),
    ],
    swiftLanguageModes: [.v6]
)
