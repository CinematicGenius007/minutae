import MinutaeDesign
import SwiftUI

/// Presentation for an active capture. The host owns the recorder and its start time.
public struct MeetingRecordingPill: View {
    private let startedAt: Date
    private let stop: () -> Void

    public init(startedAt: Date, stop: @escaping () -> Void) {
        self.startedAt = startedAt
        self.stop = stop
    }

    @MainActor
    public init(startedAt: Date, model: MeetingCaptureViewModel) {
        self.init(startedAt: startedAt, stop: { Task { await model.stop() } })
    }

    public var body: some View {
        HStack(spacing: Spacing.s12) {
            Circle()
                .fill(Color.minutae.live)
                .frame(width: Spacing.s8, height: Spacing.s8)
                .accessibilityHidden(true)
            Text("Recording")
                .font(.minutae.ui(size: 14, weight: .medium))
            Text(startedAt, style: .timer)
                .font(.minutae.mono(size: 14))
                .monospacedDigit()
            SecondaryButton("Stop", action: stop)
        }
        .foregroundStyle(Color.minutae.ink)
        .padding(Spacing.s12)
        .background(Color.minutae.surface, in: Capsule())
        .overlay(Capsule().strokeBorder(Color.minutae.line, lineWidth: 1))
        .accessibilityElement(children: .contain)
    }
}

/// A host can show this when its meeting detection service signals activity.
public struct MeetingDetectedCard: View {
    private let start: () -> Void
    private let dismiss: () -> Void

    public init(start: @escaping () -> Void, dismiss: @escaping () -> Void) {
        self.start = start
        self.dismiss = dismiss
    }

    @MainActor
    public init(model: MeetingCaptureViewModel) {
        self.init(
            start: { Task { await model.start() } },
            dismiss: { model.isPromptPresented = false }
        )
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s16) {
            VStack(alignment: .leading, spacing: Spacing.s4) {
                Text("Meeting detected")
                    .font(.minutae.display(size: 24))
                Text("Would you like to start recording?")
                    .font(.minutae.ui(size: 14))
                    .foregroundStyle(Color.minutae.muted)
            }
            HStack(spacing: Spacing.s8) {
                PrimaryButton("Start recording", action: start)
                SecondaryButton("Not now", action: dismiss)
            }
        }
        .foregroundStyle(Color.minutae.ink)
        .padding(Spacing.s20)
        .background(Color.minutae.surface, in: RoundedRectangle(cornerRadius: Radius.md))
        .overlay {
            RoundedRectangle(cornerRadius: Radius.md)
                .strokeBorder(Color.minutae.line, lineWidth: 1)
        }
    }
}

#Preview("Meeting capture") {
    VStack(spacing: Spacing.s20) {
        MeetingRecordingPill(startedAt: .now.addingTimeInterval(-90), stop: {})
        MeetingDetectedCard(start: {}, dismiss: {})
    }
    .padding(Spacing.s20)
    .background(Color.minutae.bg)
}
