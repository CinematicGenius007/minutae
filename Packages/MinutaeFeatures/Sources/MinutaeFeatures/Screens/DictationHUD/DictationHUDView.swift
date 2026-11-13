import MinutaeDesign
import SwiftUI

/// The HUD pill: ink with a coloured dot, or red for an error.
public struct DictationHUDPill: View {
    private let display: DictationHUDDisplay
    private let listeningSince: Date?

    public init(_ display: DictationHUDDisplay, listeningSince: Date? = nil) {
        self.display = display
        self.listeningSince = listeningSince
    }

    public var body: some View {
        HStack(spacing: Spacing.s8 + Spacing.s2) {
            if let dot {
                Circle()
                    .fill(dot)
                    .frame(width: 8, height: 8)
                    .accessibilityHidden(true)
            }
            Text(display.title)
                .font(.minutae.ui(size: 13))
                .foregroundStyle(Color.minutae.white)
                .lineLimit(1)
                .fixedSize()
            if display == .listening {
                ElapsedLabel(since: listeningSince ?? Date())
            }
        }
        .padding(.horizontal, Spacing.s16)
        .frame(height: 40)
        .background(fill, in: Capsule())
        .accessibilityElement(children: .combine)
        .accessibilityLabel(display.title)
    }

    private var fill: Color {
        if case .error = display { return Color.minutae.red }
        return Color.minutae.ink
    }

    private var dot: Color? {
        switch display {
        case .listening: Color.minutae.live
        case .transcribing: Color.minutae.butter
        case .inserted: Color.minutae.green
        case .copied: Color.minutae.mint
        case .error: nil
        }
    }
}

private struct ElapsedLabel: View {
    var since: Date

    var body: some View {
        TimelineView(.periodic(from: since, by: 1)) { context in
            let seconds = max(0, Int(context.date.timeIntervalSince(since)))
            Text(String(format: "%d:%02d", seconds / 60, seconds % 60))
                .font(.minutae.mono(size: 12))
                .monospacedDigit()
                .foregroundStyle(Color.minutae.white.opacity(0.7))
                .accessibilityLabel("\(seconds) seconds")
        }
    }
}

/// Root view of the HUD panel. Empty when the model has nothing to show.
struct DictationHUDContent: View {
    var model: DictationHUDModel
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        ZStack {
            if let display = model.display {
                DictationHUDPill(display, listeningSince: model.listeningSince)
                    .transition(reduceMotion ? .identity : .opacity)
            }
        }
        .animation(reduceMotion ? nil : .easeOut(duration: 0.15), value: model.display)
        .padding(Spacing.s8)
        .fixedSize()
    }
}

#Preview("HUD states") {
    VStack(alignment: .leading, spacing: Spacing.s20) {
        DictationHUDPill(.listening, listeningSince: Date().addingTimeInterval(-3))
        DictationHUDPill(.transcribing)
        DictationHUDPill(.inserted(appName: "Slack"))
        DictationHUDPill(.copied)
        DictationHUDPill(.error(DictationHUDCopy.nothingHeard))
    }
    .padding(Spacing.s44)
    .background(Color.minutae.bg)
}
