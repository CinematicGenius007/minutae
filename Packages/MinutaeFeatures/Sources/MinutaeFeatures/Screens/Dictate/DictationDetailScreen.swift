import MinutaeDesign
import SwiftUI

/// One dictation: playback, editable text, copy, export, and delete.
///
/// Paper: "14 · Dictation detail". `onBack` returns to Dictate; it is also called after a delete.
public struct DictationDetailScreen: View {
    @Bindable private var model: DictationDetailViewModel
    private let onBack: () -> Void

    public init(model: DictationDetailViewModel, onBack: @escaping () -> Void) {
        self.model = model
        self.onBack = onBack
    }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                backButton
                    .padding(.bottom, Spacing.s16)
                if model.dictation != nil {
                    content
                } else if model.hasLoaded {
                    EmptyState(
                        title: "This dictation is gone",
                        message: "It was deleted, or it was never saved.",
                        actionTitle: "Back to Dictate",
                        action: onBack
                    )
                    .frame(maxWidth: .infinity)
                    .padding(.top, Spacing.s44)
                }
            }
            .frame(maxWidth: DictateMetrics.columnWidth, alignment: .leading)
            .padding(.top, DictateMetrics.topInset)
            .padding(.horizontal, Spacing.s28)
            .padding(.bottom, Spacing.s44)
            .frame(maxWidth: .infinity)
        }
        .background(Color.minutae.bg)
        .task { await model.run() }
        .onDisappear {
            model.flush()
            model.playback.stop()
        }
        .onExitCommand(perform: onBack)
        .alert("Delete this dictation?", isPresented: $model.isConfirmingDelete) {
            Button("Delete", role: .destructive) {
                if model.delete() { onBack() }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("The text and its audio file are removed from this Mac. This can't be undone.")
        }
    }

    private var backButton: some View {
        Button(action: onBack) {
            Text("← Dictate")
                .font(.minutae.ui(size: 13))
                .foregroundStyle(Color.minutae.muted)
        }
        .buttonStyle(.plain)
        .keyboardShortcut("[", modifiers: .command)
        .accessibilityLabel("Back to Dictate")
    }

    @ViewBuilder private var content: some View {
        Text(model.title)
            .font(.minutae.display(size: 32, weight: .semibold))
            .foregroundStyle(Color.minutae.ink)
            .accessibilityAddTraits(.isHeader)
        Text(model.meta)
            .font(.minutae.mono(size: 12))
            .foregroundStyle(Color.minutae.muted)
            .padding(.top, Spacing.s4 + Spacing.s2)
        AudioPlayerPill(playback: model.playback, timeLabel: model.timeLabel)
            .padding(.top, Spacing.s20 + Spacing.s4)
        SectionLabel("Transcript · editable")
            .padding(.top, Spacing.s28)
            .padding(.bottom, Spacing.s8 + Spacing.s2)
        TranscriptEditor(
            text: Binding(get: { model.draft }, set: { model.updateDraft($0) }),
            isEditable: model.isEditable
        )
        footnote
            .padding(.top, Spacing.s8)
        actions
            .padding(.top, Spacing.s20 + Spacing.s4)
        if let message = model.errorMessage {
            Text(message)
                .font(.minutae.ui(size: 12))
                .foregroundStyle(Color.minutae.red)
                .padding(.top, Spacing.s12)
        }
    }

    private var footnote: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
            Text(
                model.isEditable
                    ? "Edits change the saved text only. The original transcript and audio stay untouched."
                    : "This dictation is still being transcribed."
            )
            .foregroundStyle(Color.minutae.muted)
            .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: Spacing.s8)
            switch model.saveState {
            case .idle: EmptyView()
            case .pending: Text("Saving…").foregroundStyle(Color.minutae.muted)
            case .saved: Text("Saved").foregroundStyle(Color.minutae.green)
            case .failed(let message): Text(message).foregroundStyle(Color.minutae.red)
            }
        }
        .font(.minutae.ui(size: 12))
    }

    private var actions: some View {
        HStack(spacing: Spacing.s8 + Spacing.s2) {
            PrimaryButton(model.notice == "Copied" ? "Copied" : "Copy text") { model.copyText() }
            SecondaryButton("Export .md") { Task { await model.exportMarkdown() } }
            SecondaryButton("Export audio .wav") { Task { await model.exportAudio() } }
                .disabled(!model.canExportAudio)
            Spacer(minLength: Spacing.s8)
            if let notice = model.notice, notice != "Copied" {
                Text(notice)
                    .font(.minutae.ui(size: 12))
                    .foregroundStyle(Color.minutae.muted)
                    .lineLimit(1)
            }
            Button("Delete") { model.requestDelete() }
                .buttonStyle(.plain)
                .font(.minutae.ui(size: 13, weight: .medium))
                .foregroundStyle(Color.minutae.red)
                .padding(.horizontal, Spacing.s16)
                .frame(height: 36)
                .disabled(!model.isEditable)
        }
        .task(id: model.notice) {
            guard model.notice != nil else { return }
            try? await Task.sleep(for: .seconds(2))
            guard !Task.isCancelled else { return }
            model.dismissNotice()
        }
    }
}

struct AudioPlayerPill: View {
    var playback: AudioPlaybackModel
    var timeLabel: String

    var body: some View {
        HStack(spacing: Spacing.s12 + Spacing.s2) {
            Button(action: playback.togglePlay) {
                Image(systemName: playback.isPlaying ? "pause.fill" : "play.fill")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(Color.minutae.white)
                    .frame(width: 36, height: 36)
                    .background(Color.minutae.pine, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .disabled(!playback.isAvailable)
            .opacity(playback.isAvailable ? 1 : 0.4)
            .accessibilityLabel(playback.isPlaying ? "Pause" : "Play")
            if let reason = playback.unavailableReason, !playback.isAvailable {
                Text(reason)
                    .font(.minutae.ui(size: 13))
                    .foregroundStyle(Color.minutae.muted)
                    .frame(maxWidth: .infinity, alignment: .leading)
            } else {
                Scrubber(playback: playback)
            }
            Text(timeLabel)
                .font(.minutae.mono(size: 12))
                .foregroundStyle(Color.minutae.muted)
                .monospacedDigit()
                .accessibilityLabel("Elapsed \(timeLabel)")
        }
        .padding(.vertical, Spacing.s12 + Spacing.s2)
        .padding(.horizontal, Spacing.s16)
        .background(Color.minutae.surface, in: Capsule())
    }
}

/// `ProgressTrack` with drag-to-seek.
struct Scrubber: View {
    var playback: AudioPlaybackModel

    var body: some View {
        GeometryReader { proxy in
            ProgressTrack(value: playback.progress)
                .frame(maxHeight: .infinity)
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { value in
                            guard proxy.size.width > 0 else { return }
                            playback.seek(to: value.location.x / proxy.size.width)
                        }
                )
        }
        .frame(height: 20)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Playback position")
        .accessibilityValue("\(Int((playback.progress * 100).rounded())) percent")
        .accessibilityAdjustableAction { direction in
            switch direction {
            case .increment: playback.skip(by: 1)
            case .decrement: playback.skip(by: -1)
            @unknown default: break
            }
        }
    }
}

/// Grows with the text. Option-Return adds a line break.
struct TranscriptEditor: View {
    @Binding var text: String
    var isEditable: Bool

    var body: some View {
        TextField("Transcript", text: $text, axis: .vertical)
            .textFieldStyle(.plain)
            .labelsHidden()
            .font(.minutae.ui(size: 16))
            .lineSpacing(Spacing.s4 + Spacing.s2)
            .lineLimit(1...12)
            .foregroundStyle(Color.minutae.ink)
            .disabled(!isEditable)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.vertical, Spacing.s16)
            .padding(.horizontal, Spacing.s16 + Spacing.s2)
            .overlay(
                RoundedRectangle(cornerRadius: Radius.md)
                    .strokeBorder(Color.minutae.line, lineWidth: 1)
            )
            .accessibilityLabel("Transcript")
    }
}

#Preview("Dictation detail") {
    DictationDetailScreen(model: DictatePreview.detailModel(), onBack: {})
        .frame(width: 968, height: 800)
}

#Preview("Dictation detail · missing") {
    DictationDetailScreen(
        model: DictationDetailViewModel(
            id: "missing",
            archive: InMemoryDictationArchive(),
            clipboard: PreviewClipboard(),
            destination: PreviewDestination(),
            makePlayer: { _ in PreviewPlayer() }
        ),
        onBack: {}
    )
    .frame(width: 968, height: 800)
}
