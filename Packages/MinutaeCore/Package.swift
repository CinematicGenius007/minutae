// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MinutaeCore",
    platforms: [.macOS("14.4")],
    products: [.library(name: "MinutaeCore", targets: ["MinutaeCore"])],
    dependencies: [.package(url: "https://github.com/groue/GRDB.swift.git", exact: "7.8.0")],
    targets: [
        .target(name: "MinutaeCore", dependencies: [.product(name: "GRDB", package: "GRDB.swift")], resources: [.process("Resources")]),
        .testTarget(name: "MinutaeCoreTests", dependencies: ["MinutaeCore", .product(name: "GRDB", package: "GRDB.swift")], resources: [.process("Fixtures")]),
    ],
    swiftLanguageModes: [.v6]
)
