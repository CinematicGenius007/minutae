import SwiftUI

/// Title and muted description on the left, a control on the right, with a hairline under the row.
public struct SettingsRow<Trailing: View>: View {
    private let title: String
    private let detail: String?
    private let trailing: Trailing

    public init(
        _ title: String,
        description: String? = nil,
        @ViewBuilder trailing: () -> Trailing
    ) {
        self.title = title
        detail = description
        self.trailing = trailing()
    }

    public var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .center, spacing: Spacing.s16) {
                VStack(alignment: .leading, spacing: Spacing.s4) {
                    Text(title)
                        .font(.minutae.ui(size: 15, weight: .medium))
                        .foregroundStyle(Color.minutae.ink)
                    if let detail {
                        Text(detail)
                            .font(.minutae.ui(size: 13))
                            .foregroundStyle(Color.minutae.muted)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                trailing
            }
            .padding(.vertical, Spacing.s12)
            HairlineDivider()
        }
    }
}

/// A list row with a trailing text action in pine.
public struct ListRow: View {
    private let title: String
    private let subtitle: String?
    private let actionTitle: String
    private let action: () -> Void

    public init(
        _ title: String,
        subtitle: String? = nil,
        actionTitle: String,
        action: @escaping () -> Void
    ) {
        self.title = title
        self.subtitle = subtitle
        self.actionTitle = actionTitle
        self.action = action
    }

    public var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .center, spacing: Spacing.s16) {
                VStack(alignment: .leading, spacing: Spacing.s4) {
                    Text(title)
                        .font(.minutae.ui(size: 15, weight: .medium))
                        .foregroundStyle(Color.minutae.ink)
                    if let subtitle {
                        Text(subtitle)
                            .font(.minutae.ui(size: 13))
                            .foregroundStyle(Color.minutae.muted)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                Button(action: action) {
                    Text(actionTitle)
                        .font(.minutae.ui(size: 13, weight: .medium))
                        .foregroundStyle(Color.minutae.pine)
                }
                .buttonStyle(.plain)
            }
            .padding(.vertical, Spacing.s12)
            HairlineDivider()
        }
    }
}

/// Centered title, muted message, and an optional primary action.
public struct EmptyState: View {
    private let title: String
    private let message: String
    private let actionTitle: String?
    private let action: (() -> Void)?

    public init(
        title: String,
        message: String,
        actionTitle: String? = nil,
        action: (() -> Void)? = nil
    ) {
        self.title = title
        self.message = message
        self.actionTitle = actionTitle
        self.action = action
    }

    public var body: some View {
        VStack(spacing: Spacing.s12) {
            Text(title)
                .font(.minutae.display(size: 28))
                .foregroundStyle(Color.minutae.ink)
                .multilineTextAlignment(.center)
            Text(message)
                .font(.minutae.ui(size: 14))
                .foregroundStyle(Color.minutae.muted)
                .multilineTextAlignment(.center)
            if let actionTitle, let action {
                PrimaryButton(actionTitle, action: action)
                    .padding(.top, Spacing.s4)
            }
        }
        .frame(maxWidth: 320)
    }
}

/// Dictation HUD. Listening is the only state that uses the live dot.
public struct HUDCapsule: View {
    private let state: HUDState

    public init(_ state: HUDState) {
        self.state = state
    }

    public var body: some View {
        HStack(spacing: Spacing.s8) {
            Circle()
                .fill(state.dot)
                .frame(width: 8, height: 8)
                .accessibilityHidden(true)
            Text(state.title)
                .font(.minutae.ui(size: 13, weight: .medium))
                .foregroundStyle(Color.minutae.white)
        }
        .padding(.horizontal, Spacing.s12)
        .padding(.vertical, Spacing.s8)
        .background(Color.minutae.ink, in: Capsule())
        .accessibilityElement(children: .combine)
        .accessibilityLabel(state.title)
    }
}

public enum HUDState: String, CaseIterable, Identifiable, Sendable {
    case idle
    case listening
    case transcribing
    case inserted
    case copied

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .idle: "Idle"
        case .listening: "Listening"
        case .transcribing: "Transcribing"
        case .inserted: "Inserted"
        case .copied: "Copied"
        }
    }

    var dot: Color {
        switch self {
        case .idle: Color.minutae.raised
        case .listening: Color.minutae.live
        case .transcribing: Color.minutae.butter
        case .inserted: Color.minutae.green
        case .copied: Color.minutae.mint
        }
    }
}

#Preview("Settings row") {
    SettingsRowPreview()
        .padding(Spacing.s16)
        .frame(width: 520)
        .background(Color.minutae.bg)
}

#Preview("List row") {
    ListRow("Roadmap review", subtitle: "Today · 24 min", actionTitle: "Open") {}
        .padding(.horizontal, Spacing.s16)
        .frame(width: 520)
        .background(Color.minutae.bg)
}

#Preview("Empty state") {
    EmptyState(
        title: "Nothing here yet",
        message: "A meeting you record will show up in the library.",
        actionTitle: "Record a meeting"
    ) {}
    .padding(Spacing.s28)
    .frame(maxWidth: .infinity)
    .background(Color.minutae.bg)
}

#Preview("HUD capsule") {
    HStack(spacing: Spacing.s8) {
        ForEach(HUDState.allCases) { state in
            HUDCapsule(state)
        }
    }
    .padding(Spacing.s16)
    .background(Color.minutae.bg)
}

private struct SettingsRowPreview: View {
    @State private var keepAudio = true

    var body: some View {
        VStack(spacing: 0) {
            SettingsRow("Keep audio", description: "Save every dictation on this Mac.") {
                Toggle("Keep audio", isOn: $keepAudio)
                    .toggleStyle(.minutaePine)
                    .labelsHidden()
                    .accessibilityLabel("Keep audio")
            }
            SettingsRow("Hotkey", description: "Hold to dictate into the frontmost app.") {
                KeyCap("⌥Space", accessibilityText: "Option Space")
            }
        }
    }
}
