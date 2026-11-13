import SwiftUI

extension Font {
    public enum Minutae: Sendable {
        public static func display(
            size: CGFloat,
            weight: Font.Weight = .regular,
            italic: Bool = false
        ) -> Font {
            MinutaeFont.display(size: size, weight: weight, italic: italic)
        }

        public static func ui(size: CGFloat, weight: Font.Weight = .regular) -> Font {
            MinutaeFont.ui(size: size, weight: weight)
        }

        public static func mono(size: CGFloat, weight: Font.Weight = .regular) -> Font {
            MinutaeFont.mono(size: size, weight: weight)
        }
    }

    public static var minutae: Minutae.Type { Minutae.self }
}

enum MinutaeFont {
    static func display(size: CGFloat, weight: Font.Weight, italic: Bool) -> Font {
        let faces = FontBook.shared.snapshot()
        if italic {
            if let family = faces.italicFamily {
                return face(
                    family: family,
                    size: size,
                    weight: weight,
                    italic: faces.italicNeedsTrait,
                    design: .serif
                )
            }
            return face(family: nil, size: size, weight: weight, italic: true, design: .serif)
        }
        return face(
            family: faces.displayFamily,
            size: size,
            weight: weight,
            italic: false,
            design: .serif
        )
    }

    static func ui(size: CGFloat, weight: Font.Weight) -> Font {
        face(
            family: FontBook.shared.snapshot().uiFamily,
            size: size,
            weight: weight,
            italic: false,
            design: .default
        )
    }

    static func mono(size: CGFloat, weight: Font.Weight) -> Font {
        face(
            family: FontBook.shared.snapshot().monoFamily,
            size: size,
            weight: weight,
            italic: false,
            design: .monospaced
        )
    }

    static func face(
        family: String?,
        size: CGFloat,
        weight: Font.Weight,
        italic: Bool,
        design: Font.Design
    ) -> Font {
        guard let family else {
            var font = Font.system(size: size, weight: weight, design: design)
            if italic { font = font.italic() }
            return font
        }
        var font = Font.custom(family, size: size, relativeTo: textStyle(for: size))
        if weight != .regular { font = font.weight(weight) }
        if italic { font = font.italic() }
        return font
    }

    static func textStyle(for size: CGFloat) -> Font.TextStyle {
        if size >= 34 { return .largeTitle }
        if size >= 26 { return .title }
        if size >= 20 { return .title2 }
        if size >= 17 { return .title3 }
        if size >= 15 { return .body }
        if size >= 13 { return .callout }
        if size >= 12 { return .footnote }
        if size >= 11 { return .caption }
        return .caption2
    }
}
