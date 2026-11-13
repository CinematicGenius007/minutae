import Foundation
import SwiftUI
import Testing

@testable import MinutaeDesign

@Test func registerFontsLoadsBundledFaces() {
    let result = MinutaeDesign.registerFonts()
    #expect(result.missing.isEmpty)
    #expect(result.failed.isEmpty)
    #expect(
        result.registered == [
            "Fraunces-Variable.ttf",
            "Fraunces-Italic-Variable.ttf",
            "Geist-Variable.ttf",
            "GeistMono-Variable.ttf",
        ]
    )
    #expect(result.displayFamily == "Fraunces")
    #expect(result.uiFamily == "Geist")
    #expect(result.monoFamily == "Geist Mono")
    #expect(result.italicReady)
    let again = MinutaeDesign.registerFonts()
    #expect(again == result)
}

@Test func registeringTwiceTreatsAlreadyRegisteredAsSuccess() {
    let urls = FontBook.bundledFontURLs().values.sorted {
        $0.lastPathComponent < $1.lastPathComponent
    }
    #expect(urls.count == 4)
    let first = FontRegistrar.register(urls: urls)
    let second = FontRegistrar.register(urls: urls)
    #expect(first.failed.isEmpty)
    #expect(first.missing.isEmpty)
    #expect(second.failed.isEmpty)
    #expect(second.missing.isEmpty)
    #expect(second.registered.count == 4)
}

@Test func missingAndInvalidFontsDoNotCrash() throws {
    let directory = FileManager.default.temporaryDirectory.appending(
        path: "minutae-fonts-\(UUID().uuidString)",
        directoryHint: .isDirectory
    )
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: directory) }

    let missing = directory.appending(path: "Missing.ttf")
    let garbage = directory.appending(path: "Garbage.ttf")
    try Data([0, 1, 2, 3, 4, 5]).write(to: garbage)

    let result = FontRegistrar.register(urls: [missing, garbage, directory])
    #expect(result.missing == ["Missing.ttf", directory.lastPathComponent])
    #expect(result.failed == ["Garbage.ttf"])
    #expect(result.registered.isEmpty)

    _ = MinutaeFont.face(family: nil, size: 18, weight: .regular, italic: true, design: .serif)
    _ = MinutaeFont.face(family: nil, size: 14, weight: .medium, italic: false, design: .default)
    _ = MinutaeFont.face(
        family: nil, size: 12, weight: .regular, italic: false, design: .monospaced)
    _ = Font.minutae.display(size: 44, italic: true)
    _ = Font.minutae.ui(size: 14, weight: .medium)
    _ = Font.minutae.mono(size: 13)
}

@Test func emptyRegistrationDoesNotCrash() {
    let result = FontRegistrar.register(urls: [])
    #expect(result.registered.isEmpty)
    #expect(result.missing.isEmpty)
    #expect(result.failed.isEmpty)
}

@Test func openFontLicensesAreBundled() throws {
    let urls = FontBook.licenseURLs()
    let names = Set(urls.map(\.lastPathComponent))
    #expect(names.contains("Fraunces-OFL.txt"))
    #expect(names.contains("Geist-OFL.txt"))
    #expect(names.contains("GeistMono-OFL.txt"))
    for url in urls {
        let text = try String(contentsOf: url, encoding: .utf8)
        #expect(text.contains("SIL Open Font License"))
    }
}
