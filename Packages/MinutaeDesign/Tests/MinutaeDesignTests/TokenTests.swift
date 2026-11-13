import SwiftUI
import Testing

@testable import MinutaeDesign

@Test func paperHexValues() {
    let expected = [
        ExpectedHex(name: "bg", hex: 0xF7F3EA),
        ExpectedHex(name: "surface", hex: 0xEFE9DA),
        ExpectedHex(name: "raised", hex: 0xE6DFCD),
        ExpectedHex(name: "line", hex: 0xDDD5C0),
        ExpectedHex(name: "ink", hex: 0x1B1A17),
        ExpectedHex(name: "muted", hex: 0x5A5648),
        ExpectedHex(name: "pine", hex: 0x1F4D3A),
        ExpectedHex(name: "mint", hex: 0xCBEBD6),
        ExpectedHex(name: "live", hex: 0xE5442B),
        ExpectedHex(name: "green", hex: 0x2E8B57),
        ExpectedHex(name: "red", hex: 0xC8321F),
        ExpectedHex(name: "butter", hex: 0xFFD95A),
        ExpectedHex(name: "lilac", hex: 0xD9CCFF),
        ExpectedHex(name: "peach", hex: 0xFFD3BF),
    ]
    let actual = MinutaePalette.paper
    #expect(actual.count == expected.count)
    for index in expected.indices {
        #expect(actual[index].name == expected[index].name)
        #expect(actual[index].hex == expected[index].hex)
    }
    #expect(MinutaePalette.white.hex == 0xFFFFFF)
    #expect(MinutaePalette.pine.paperName == "cobalt")
    #expect(MinutaePalette.mint.paperName == "cobalt-soft")
}

private struct ExpectedHex {
    var name: String
    var hex: UInt32
}

@Test func spacingRadiusAndLayout() {
    #expect(Spacing.all == [2, 4, 8, 12, 16, 20, 28, 44])
    #expect(Radius.sm == 8)
    #expect(Radius.md == 12)
    #expect(Layout.windowWidth == 1200)
    #expect(Layout.windowHeight == 800)
    #expect(Layout.sidebarWidth == 232)
}

@Test func colorFacadeMatchesPalette() {
    #expect(Color.minutae.bg == MinutaePalette.bg.color)
    #expect(Color.minutae.surface == MinutaePalette.surface.color)
    #expect(Color.minutae.raised == MinutaePalette.raised.color)
    #expect(Color.minutae.ink == MinutaePalette.ink.color)
    #expect(Color.minutae.muted == MinutaePalette.muted.color)
    #expect(Color.minutae.pine == MinutaePalette.pine.color)
    #expect(Color.minutae.mint == MinutaePalette.mint.color)
    #expect(Color.minutae.live == MinutaePalette.live.color)
    #expect(Color.minutae.white == MinutaePalette.white.color)
}
