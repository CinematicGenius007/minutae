import SwiftUI

/// Pine pill with white text. The main action on a page.
public struct PrimaryButton: View {
    private let title: String
    private let action: () -> Void

    public init(_ title: String, action: @escaping () -> Void) {
        self.title = title
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            Text(title)
        }
        .buttonStyle(PrimaryPillStyle())
    }
}

/// Outlined pill. Ink text on the page colour, ink stroke.
public struct SecondaryButton: View {
    private let title: String
    private let action: () -> Void

    public init(_ title: String, action: @escaping () -> Void) {
        self.title = title
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            Text(title)
        }
        .buttonStyle(SecondaryPillStyle())
    }
}

/// Pine pill with a capture dot and a shortcut hint.
///
/// The dot uses `live` only while `isCapturing` is true.
public struct RecordButton: View {
    private let title: String
    private let shortcut: String
    private let isCapturing: Bool
    private let action: () -> Void

    public init(
        _ title: String = "Record",
        shortcut: String = "⌥Space",
        isCapturing: Bool = false,
        action: @escaping () -> Void
    ) {
        self.title = title
        self.shortcut = shortcut
        self.isCapturing = isCapturing
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            RecordButtonLabel(
                title: title,
                shortcut: shortcut,
                shortcutLabel: ShortcutSpeech.label(for: shortcut),
                isCapturing: isCapturing
            )
        }
        .buttonStyle(PressedFadeStyle())
        .accessibilityLabel(isCapturing ? "\(title), capturing audio" : title)
        .accessibilityHint(ShortcutSpeech.label(for: shortcut))
    }
}

private struct PrimaryPillStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        PillLabel(
            isPressed: configuration.isPressed,
            fill: Color.minutae.pine,
            foreground: Color.minutae.white,
            stroke: nil,
            label: configuration.label
        )
    }
}

private struct SecondaryPillStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        PillLabel(
            isPressed: configuration.isPressed,
            fill: Color.clear,
            foreground: Color.minutae.ink,
            stroke: Color.minutae.ink,
            label: configuration.label
        )
    }
}

private struct PressedFadeStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label.opacity(configuration.isPressed ? 0.82 : 1)
    }
}

private struct PillLabel<Label: View>: View {
    @Environment(\.isEnabled) private var isEnabled
    var isPressed: Bool
    var fill: Color
    var foreground: Color
    var stroke: Color?
    var label: Label

    var body: some View {
        label
            .font(.minutae.ui(size: 14, weight: .medium))
            .foregroundStyle(foreground)
            .padding(.horizontal, Spacing.s16)
            .padding(.vertical, Spacing.s8)
            .background(fillColor, in: Capsule())
            .overlay {
                if let stroke {
                    Capsule().strokeBorder(stroke, lineWidth: 1)
                }
            }
            .opacity(isEnabled ? 1 : 0.4)
    }

    private var fillColor: Color {
        guard isPressed else { return fill }
        if stroke == nil { return fill.opacity(0.82) }
        return Color.minutae.raised
    }
}

private struct RecordButtonLabel: View {
    @Environment(\.isEnabled) private var isEnabled
    var title: String
    var shortcut: String
    var shortcutLabel: String
    var isCapturing: Bool

    var body: some View {
        HStack(spacing: Spacing.s8) {
            Circle()
                .fill(isCapturing ? Color.minutae.live : Color.minutae.white.opacity(0.45))
                .frame(width: 8, height: 8)
                .accessibilityHidden(true)
            Text(title)
                .font(.minutae.ui(size: 14, weight: .medium))
                .foregroundStyle(Color.minutae.white)
            KeyCap(shortcut, accessibilityText: shortcutLabel)
        }
        .padding(.horizontal, Spacing.s16)
        .padding(.vertical, Spacing.s8)
        .background(Color.minutae.pine, in: Capsule())
        .opacity(isEnabled ? 1 : 0.4)
    }
}

#Preview("Primary button") {
    VStack(alignment: .leading, spacing: Spacing.s12) {
        PrimaryButton("Continue") {}
        PrimaryButton("Continue") {}.disabled(true)
    }
    .padding(Spacing.s16)
    .background(Color.minutae.bg)
}

#Preview("Secondary button") {
    VStack(alignment: .leading, spacing: Spacing.s12) {
        SecondaryButton("Cancel") {}
        SecondaryButton("Cancel") {}.disabled(true)
    }
    .padding(Spacing.s16)
    .background(Color.minutae.bg)
}

#Preview("Record button") {
    VStack(alignment: .leading, spacing: Spacing.s12) {
        RecordButton {}
        RecordButton(isCapturing: true) {}
        RecordButton {}.disabled(true)
    }
    .padding(Spacing.s16)
    .background(Color.minutae.bg)
}
