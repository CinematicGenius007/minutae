import SwiftUI

enum ShortcutSpeech {
    static func label(for shortcut: String) -> String {
        switch shortcut {
        case "⌥Space": "Option Space"
        case "⌘K": "Command K"
        default: shortcut
        }
    }
}

/// Pill search field with a shortcut hint, defaulting to ⌘K.
public struct SearchField: View {
    @Binding private var text: String
    private let placeholder: String
    private let shortcut: String
    private let shortcutLabel: String

    public init(
        text: Binding<String>,
        placeholder: String = "Search",
        shortcut: String = "⌘K",
        shortcutLabel: String = "Command K"
    ) {
        _text = text
        self.placeholder = placeholder
        self.shortcut = shortcut
        self.shortcutLabel = shortcutLabel
    }

    public var body: some View {
        HStack(spacing: Spacing.s8) {
            Image(systemName: "magnifyingglass")
                .font(.minutae.ui(size: 14, weight: .medium))
                .foregroundStyle(Color.minutae.muted)
                .accessibilityHidden(true)
            ZStack(alignment: .leading) {
                if text.isEmpty {
                    Text(placeholder)
                        .font(.minutae.ui(size: 14))
                        .foregroundStyle(Color.minutae.muted)
                        .accessibilityHidden(true)
                }
                TextField("", text: $text)
                    .textFieldStyle(.plain)
                    .font(.minutae.ui(size: 14))
                    .foregroundStyle(Color.minutae.ink)
                    .accessibilityLabel(placeholder)
            }
            KeyCap(shortcut, accessibilityText: shortcutLabel)
        }
        .padding(.horizontal, Spacing.s12)
        .padding(.vertical, Spacing.s8)
        .background(Color.minutae.surface, in: Capsule())
        .overlay(Capsule().strokeBorder(Color.minutae.line, lineWidth: 1))
    }
}

/// Small bordered key, such as ⌥Space.
public struct KeyCap: View {
    private let text: String
    private let accessibilityText: String?

    public init(_ text: String, accessibilityText: String? = nil) {
        self.text = text
        self.accessibilityText = accessibilityText
    }

    public var body: some View {
        Text(text)
            .font(.minutae.mono(size: 12, weight: .medium))
            .foregroundStyle(Color.minutae.ink)
            .padding(.horizontal, Spacing.s8)
            .padding(.vertical, Spacing.s4)
            .background(Color.minutae.surface, in: RoundedRectangle(cornerRadius: Spacing.s4))
            .overlay {
                RoundedRectangle(cornerRadius: Spacing.s4)
                    .strokeBorder(Color.minutae.line, lineWidth: 1)
            }
            .accessibilityLabel(accessibilityText ?? text)
    }
}

/// Pine when on, raised track with an ink knob when off.
public struct PineToggleStyle: ToggleStyle {
    public init() {}

    public func makeBody(configuration: Configuration) -> some View {
        Button {
            configuration.isOn.toggle()
        } label: {
            HStack(spacing: Spacing.s12) {
                configuration.label
                ToggleTrack(isOn: configuration.isOn)
            }
        }
        .buttonStyle(.plain)
        .accessibilityValue(configuration.isOn ? "On" : "Off")
        .accessibilityAddTraits(.isToggle)
    }
}

extension ToggleStyle where Self == PineToggleStyle {
    public static var minutaePine: PineToggleStyle { PineToggleStyle() }
}

private struct ToggleTrack: View {
    var isOn: Bool

    var body: some View {
        Capsule()
            .fill(isOn ? Color.minutae.pine : Color.minutae.raised)
            .frame(width: 40, height: 24)
            .overlay(alignment: isOn ? .trailing : .leading) {
                Circle()
                    .fill(isOn ? Color.minutae.white : Color.minutae.ink)
                    .frame(width: 18, height: 18)
                    .padding(.horizontal, 3)
            }
            .animation(.easeOut(duration: 0.15), value: isOn)
            .accessibilityHidden(true)
    }
}

/// A row of pills. The selected pill is pine with white text.
public struct SegmentedPills<ID: Hashable>: View {
    private let options: [ID]
    private let title: (ID) -> String
    @Binding private var selection: ID

    public init(
        options: [ID],
        selection: Binding<ID>,
        title: @escaping (ID) -> String
    ) {
        self.options = options
        self.title = title
        _selection = selection
    }

    public var body: some View {
        HStack(spacing: Spacing.s4) {
            ForEach(options, id: \.self) { option in
                let selected = option == selection
                Button {
                    selection = option
                } label: {
                    Text(title(option))
                        .font(.minutae.ui(size: 13, weight: .medium))
                        .foregroundStyle(selected ? Color.minutae.white : Color.minutae.ink)
                        .padding(.horizontal, Spacing.s12)
                        .padding(.vertical, Spacing.s8)
                        .background(selected ? Color.minutae.pine : Color.clear, in: Capsule())
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(selected ? .isSelected : [])
            }
        }
        .padding(Spacing.s4)
        .background(Color.minutae.raised, in: Capsule())
    }
}

extension SegmentedPills where ID == String {
    public init(options: [String], selection: Binding<String>) {
        self.init(options: options, selection: selection, title: { $0 })
    }
}

/// Pine fill on a raised track. `value` is clamped to 0...1.
public struct ProgressTrack: View {
    private let value: Double

    public init(value: Double) {
        self.value = min(max(value, 0), 1)
    }

    public var body: some View {
        Capsule()
            .fill(Color.minutae.raised)
            .frame(height: 6)
            .overlay(alignment: .leading) {
                GeometryReader { proxy in
                    Capsule()
                        .fill(Color.minutae.pine)
                        .frame(width: proxy.size.width * value)
                }
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Progress")
            .accessibilityValue("\(Int((value * 100).rounded())) percent")
    }
}

#Preview("Search field") {
    SearchFieldPreview()
}

#Preview("Key cap") {
    KeyCap("⌥Space", accessibilityText: "Option Space")
        .padding(Spacing.s16)
        .background(Color.minutae.bg)
}

#Preview("Pine toggle") {
    PineTogglePreview()
}

#Preview("Segmented pills") {
    SegmentedPillsPreview()
}

#Preview("Progress track") {
    VStack(spacing: Spacing.s12) {
        ProgressTrack(value: 0.35)
        ProgressTrack(value: 1)
    }
    .padding(Spacing.s16)
    .frame(width: 280)
    .background(Color.minutae.bg)
}

private struct SearchFieldPreview: View {
    @State private var empty = ""
    @State private var filled = "quarterly plan"

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s12) {
            SearchField(text: $empty)
            SearchField(text: $filled)
        }
        .padding(Spacing.s16)
        .frame(width: 360)
        .background(Color.minutae.bg)
    }
}

private struct PineTogglePreview: View {
    @State private var isOn = true

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s12) {
            Toggle("Keep audio", isOn: $isOn)
                .toggleStyle(.minutaePine)
            Toggle("Launch at login", isOn: .constant(false))
                .toggleStyle(.minutaePine)
        }
        .font(.minutae.ui(size: 14))
        .foregroundStyle(Color.minutae.ink)
        .padding(Spacing.s16)
        .frame(width: 280)
        .background(Color.minutae.bg)
    }
}

private struct SegmentedPillsPreview: View {
    @State private var selection = "Meetings"

    var body: some View {
        SegmentedPills(options: ["Meetings", "Notes", "Dictations"], selection: $selection)
            .padding(Spacing.s16)
            .background(Color.minutae.bg)
    }
}
