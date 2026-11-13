// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeLLM",
    platforms: [.macOS("14.4")],
    products: [.library(name: "MinutaeLLM", targets: ["MinutaeLLM"])],
    dependencies: [.package(path: "../MinutaeCore")],
    targets: [
        .target(name: "MinutaeLLM", dependencies: [.product(name: "MinutaeCore", package: "MinutaeCore")]),
        .testTarget(name: "MinutaeLLMTests", dependencies: ["MinutaeLLM"]),
    ],
    swiftLanguageModes: [.v6]
)
