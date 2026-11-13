import Foundation
import SwiftUI

// Paper names are short on purpose: bg, sm, s8.
// swiftlint:disable identifier_name

/// One colour from the Ink & signal sheet.
public struct MinutaeColorToken: Identifiable, Equatable, Sendable {
    public let name: String
    public let hex: UInt32
    /// Name in the Paper file when it differs from `name`.
    public let paperName: String?

    public var id: String { name }

    public var color: Color { Color(hex: hex) }

    public var hexString: String { String(format: "#%06X", hex) }

    init(name: String, hex: UInt32, paperName: String? = nil) {
        self.name = name
        self.hex = hex
        self.paperName = paperName
    }
}

/// Approved light-theme colours. `Color.minutae` reads these values.
public enum MinutaePalette: Sendable {
    public static let bg = MinutaeColorToken(name: "bg", hex: 0xF7F3EA)
    public static let surface = MinutaeColorToken(name: "surface", hex: 0xEFE9DA)
    public static let raised = MinutaeColorToken(name: "raised", hex: 0xE6DFCD)
    public static let line = MinutaeColorToken(name: "line", hex: 0xDDD5C0)
    public static let ink = MinutaeColorToken(name: "ink", hex: 0x1B1A17)
    public static let muted = MinutaeColorToken(name: "muted", hex: 0x5A5648)
    /// Primary action. Paper calls this cobalt.
    public static let pine = MinutaeColorToken(name: "pine", hex: 0x1F4D3A, paperName: "cobalt")
    /// Wash. Paper calls this cobalt-soft.
    public static let mint = MinutaeColorToken(
        name: "mint", hex: 0xCBEBD6, paperName: "cobalt-soft")
    /// Capture indicator only. Do not use it for text or for idle chrome.
    public static let live = MinutaeColorToken(name: "live", hex: 0xE5442B)
    public static let green = MinutaeColorToken(name: "green", hex: 0x2E8B57)
    public static let red = MinutaeColorToken(name: "red", hex: 0xC8321F)
    public static let butter = MinutaeColorToken(name: "butter", hex: 0xFFD95A)
    public static let lilac = MinutaeColorToken(name: "lilac", hex: 0xD9CCFF)
    public static let peach = MinutaeColorToken(name: "peach", hex: 0xFFD3BF)
    /// Inverse text on pine, red, and the HUD.
    public static let white = MinutaeColorToken(name: "white", hex: 0xFFFFFF)

    /// The Paper sheet, in the approved order. White is inverse text, not a Paper swatch.
    public static let paper: [MinutaeColorToken] = [
        bg, surface, raised, line, ink, muted, pine, mint, live, green, red, butter, lilac, peach,
    ]

    public static let all: [MinutaeColorToken] = paper + [white]
}

extension Color {
    public enum Minutae: Sendable {
        public static let bg = MinutaePalette.bg.color
        public static let surface = MinutaePalette.surface.color
        public static let raised = MinutaePalette.raised.color
        public static let line = MinutaePalette.line.color
        public static let ink = MinutaePalette.ink.color
        public static let muted = MinutaePalette.muted.color
        public static let pine = MinutaePalette.pine.color
        public static let mint = MinutaePalette.mint.color
        public static let live = MinutaePalette.live.color
        public static let green = MinutaePalette.green.color
        public static let red = MinutaePalette.red.color
        public static let butter = MinutaePalette.butter.color
        public static let lilac = MinutaePalette.lilac.color
        public static let peach = MinutaePalette.peach.color
        public static let white = MinutaePalette.white.color
    }

    public static var minutae: Minutae.Type { Minutae.self }
}

extension Color {
    init(hex: UInt32) {
        let red = Double((hex >> 16) & 0xFF) / 255
        let green = Double((hex >> 8) & 0xFF) / 255
        let blue = Double(hex & 0xFF) / 255
        self.init(.sRGB, red: red, green: green, blue: blue, opacity: 1)
    }
}

/// 8 and 12. Pills use `Capsule()` so they stay fully rounded at any height.
public enum Radius: Sendable {
    public static let sm: CGFloat = 8
    public static let md: CGFloat = 12
}

/// The spacing scale: 2, 4, 8, 12, 16, 20, 28, 44.
public enum Spacing: Sendable {
    public static let s2: CGFloat = 2
    public static let s4: CGFloat = 4
    public static let s8: CGFloat = 8
    public static let s12: CGFloat = 12
    public static let s16: CGFloat = 16
    public static let s20: CGFloat = 20
    public static let s28: CGFloat = 28
    public static let s44: CGFloat = 44

    public static let all: [CGFloat] = [s2, s4, s8, s12, s16, s20, s28, s44]
}

/// The Ink & signal window.
public enum Layout: Sendable {
    public static let windowWidth: CGFloat = 1200
    public static let windowHeight: CGFloat = 800
    public static let sidebarWidth: CGFloat = 232
}

// swiftlint:enable identifier_name
