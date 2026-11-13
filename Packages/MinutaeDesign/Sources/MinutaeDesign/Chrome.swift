import SwiftUI

/// One sidebar row. The selected row is a butter fill. Hover uses raised.
public struct SidebarItem: View {
    private let title: String
    private let systemImage: String?
    private let isSelected: Bool
    private let action: () -> Void
    @State private var isHovering = false

    public init(
        _ title: String,
        systemImage: String? = nil,
        isSelected: Bool,
        action: @escaping () -> Void
    ) {
        self.title = title
        self.systemImage = systemImage
        self.isSelected = isSelected
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            HStack(spacing: Spacing.s8) {
                if let systemImage {
                    Image(systemName: systemImage)
                        .font(.minutae.ui(size: 14, weight: .medium))
                        .accessibilityHidden(true)
                }
                Text(title)
                    .font(.minutae.ui(size: 14, weight: .medium))
                Spacer(minLength: 0)
            }
            .foregroundStyle(Color.minutae.ink)
            .padding(.horizontal, Spacing.s12)
            .padding(.vertical, Spacing.s8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(fill, in: RoundedRectangle(cornerRadius: Radius.md))
            .contentShape(RoundedRectangle(cornerRadius: Radius.md))
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(isSelected ? .isSelected : [])
        .onHover { isHovering = $0 }
    }

    private var fill: Color {
        if isSelected { return Color.minutae.butter }
        if isHovering { return Color.minutae.raised }
        return Color.clear
    }
}

/// Small uppercase tracked label in muted.
public struct SectionLabel: View {
    private let text: String

    public init(_ text: String) {
        self.text = text
    }

    public var body: some View {
        Text(text)
            .font(.minutae.ui(size: 11, weight: .semibold))
            .tracking(0.8)
            .textCase(.uppercase)
            .foregroundStyle(Color.minutae.muted)
            .accessibilityAddTraits(.isHeader)
    }
}

/// Fraunces title. `emphasis` is italic, as in "Just say it".
public struct PageTitle: View {
    private let leading: String
    private let emphasis: String
    private let trailing: String

    public init(_ leading: String, emphasis: String, trailing: String = "") {
        self.leading = leading
        self.emphasis = emphasis
        self.trailing = trailing
    }

    public var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 0) {
            Text(leading)
                .font(.minutae.display(size: 44))
            Text(emphasis)
                .font(.minutae.display(size: 44, italic: true))
            Text(trailing)
                .font(.minutae.display(size: 44))
        }
        .foregroundStyle(Color.minutae.ink)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(leading + emphasis + trailing)
        .accessibilityAddTraits(.isHeader)
    }
}

/// One-point rule in the line colour.
public struct HairlineDivider: View {
    public init() {}

    public var body: some View {
        Rectangle()
            .fill(Color.minutae.line)
            .frame(height: 1)
            .accessibilityHidden(true)
    }
}

/// Status chip. Mint and butter use ink text. Red uses white text.
public struct StatusPill: View {
    private let title: String
    private let tone: StatusPillTone

    public init(_ title: String, tone: StatusPillTone) {
        self.title = title
        self.tone = tone
    }

    public var body: some View {
        FillPill(title: title, fill: tone.fill, foreground: tone.foreground)
    }
}

public enum StatusPillTone: String, CaseIterable, Sendable {
    case mint
    case butter
    case red

    var fill: Color {
        switch self {
        case .mint: Color.minutae.mint
        case .butter: Color.minutae.butter
        case .red: Color.minutae.red
        }
    }

    var foreground: Color {
        switch self {
        case .mint, .butter: Color.minutae.ink
        case .red: Color.minutae.white
        }
    }
}

/// Category chip. Mint, lilac, and peach all use ink text.
public struct Chip: View {
    private let title: String
    private let tone: ChipTone

    public init(_ title: String, tone: ChipTone) {
        self.title = title
        self.tone = tone
    }

    public var body: some View {
        FillPill(title: title, fill: tone.fill, foreground: Color.minutae.ink)
    }
}

public enum ChipTone: String, CaseIterable, Sendable {
    case mint
    case lilac
    case peach

    var fill: Color {
        switch self {
        case .mint: Color.minutae.mint
        case .lilac: Color.minutae.lilac
        case .peach: Color.minutae.peach
        }
    }
}

struct FillPill: View {
    var title: String
    var fill: Color
    var foreground: Color

    var body: some View {
        Text(title)
            .font(.minutae.ui(size: 12, weight: .medium))
            .foregroundStyle(foreground)
            .padding(.horizontal, Spacing.s12)
            .padding(.vertical, Spacing.s4)
            .background(fill, in: Capsule())
    }
}

#Preview("Sidebar item") {
    VStack(alignment: .leading, spacing: Spacing.s4) {
        SidebarItem("Meetings", systemImage: "calendar", isSelected: true) {}
        SidebarItem("Dictate", systemImage: "waveform", isSelected: false) {}
    }
    .padding(Spacing.s16)
    .frame(width: Layout.sidebarWidth)
    .background(Color.minutae.surface)
}

#Preview("Section label") {
    SectionLabel("Recording")
        .padding(Spacing.s16)
        .background(Color.minutae.bg)
}

#Preview("Page title") {
    PageTitle("Just ", emphasis: "say", trailing: " it")
        .padding(Spacing.s16)
        .background(Color.minutae.bg)
}

#Preview("Hairline") {
    HairlineDivider()
        .padding(Spacing.s16)
        .background(Color.minutae.bg)
}

#Preview("Status pill") {
    HStack(spacing: Spacing.s8) {
        StatusPill("Listening for meetings", tone: .mint)
        StatusPill("Selected", tone: .butter)
        StatusPill("Failed", tone: .red)
    }
    .padding(Spacing.s16)
    .background(Color.minutae.bg)
}

#Preview("Chip") {
    HStack(spacing: Spacing.s8) {
        Chip("Meeting", tone: .mint)
        Chip("Note", tone: .lilac)
        Chip("Dictation", tone: .peach)
    }
    .padding(Spacing.s16)
    .background(Color.minutae.bg)
}
