import Foundation
import Testing

@testable import MinutaeDesign

/// WCAG 2.1 relative luminance and contrast, computed here so the check is not a stub.
private func contrastRatio(_ foreground: UInt32, _ background: UInt32) -> Double {
    let lighter = max(relativeLuminance(foreground), relativeLuminance(background))
    let darker = min(relativeLuminance(foreground), relativeLuminance(background))
    return (lighter + 0.05) / (darker + 0.05)
}

private func relativeLuminance(_ hex: UInt32) -> Double {
    let red = linearChannel((hex >> 16) & 0xFF)
    let green = linearChannel((hex >> 8) & 0xFF)
    let blue = linearChannel(hex & 0xFF)
    return (0.2126 * red) + (0.7152 * green) + (0.0722 * blue)
}

private func linearChannel(_ value: UInt32) -> Double {
    let unit = Double(value) / 255
    if unit <= 0.04045 { return unit / 12.92 }
    return pow((unit + 0.055) / 1.055, 2.4)
}

@Test func contrastFormulaMatchesKnownPair() {
    let ratio = contrastRatio(MinutaePalette.ink.hex, MinutaePalette.bg.hex)
    #expect(ratio > 15.5)
    #expect(ratio < 16.0)
}

@Test func textPairsMeetContrast() {
    let ink = MinutaePalette.ink.hex
    let muted = MinutaePalette.muted.hex
    let white = MinutaePalette.white.hex
    let pine = MinutaePalette.pine.hex
    let pairs = [
        TextPair(
            name: "ink on bg", foreground: ink, background: MinutaePalette.bg.hex, minimum: 4.5),
        TextPair(
            name: "ink on surface", foreground: ink, background: MinutaePalette.surface.hex,
            minimum: 4.5),
        TextPair(
            name: "ink on raised", foreground: ink, background: MinutaePalette.raised.hex,
            minimum: 4.5),
        TextPair(
            name: "ink on mint", foreground: ink, background: MinutaePalette.mint.hex, minimum: 4.5),
        TextPair(
            name: "ink on butter", foreground: ink, background: MinutaePalette.butter.hex,
            minimum: 4.5),
        TextPair(
            name: "ink on lilac", foreground: ink, background: MinutaePalette.lilac.hex,
            minimum: 4.5),
        TextPair(
            name: "ink on peach", foreground: ink, background: MinutaePalette.peach.hex,
            minimum: 4.5),
        TextPair(name: "white on pine", foreground: white, background: pine, minimum: 4.5),
        TextPair(
            name: "white on red", foreground: white, background: MinutaePalette.red.hex,
            minimum: 4.5),
        TextPair(name: "white on ink", foreground: white, background: ink, minimum: 4.5),
        TextPair(
            name: "pine on bg", foreground: pine, background: MinutaePalette.bg.hex, minimum: 4.5),
        TextPair(
            name: "muted on bg", foreground: muted, background: MinutaePalette.bg.hex, minimum: 5.5),
        TextPair(
            name: "muted on surface", foreground: muted, background: MinutaePalette.surface.hex,
            minimum: 5.5),
        TextPair(
            name: "muted on raised", foreground: muted, background: MinutaePalette.raised.hex,
            minimum: 5.5),
    ]
    for pair in pairs {
        let ratio = contrastRatio(pair.foreground, pair.background)
        #expect(ratio >= pair.minimum, "\(pair.name) is \(ratio), needs \(pair.minimum)")
    }
}

private struct TextPair {
    var name: String
    var foreground: UInt32
    var background: UInt32
    var minimum: Double
}
