import MinutaeDesign
import SwiftUI

/// "Just say it": the mic, the hotkey hint, and RECENT dictations.
///
/// Paper: "07 · Dictate (v2: ⌥Space, audio kept)". Space toggles a dictation while the screen
/// has focus. A row click calls `onOpen` with the dictation id.
public struct DictateScreen: View {
    @Bindable private var model: DictateViewModel
    private let hotkeyLabel: String
    private let onOpen: (String) -> Void
    @FocusState private var isFocused: Bool

    public init(
        model: DictateViewModel,
        hotkeyLabel: String = "⌥ Space",
        onOpen: @escaping (String) -> Void
    ) {
        self.model = model
        self.hotkeyLabel = hotkeyLabel
        self.onOpen = onOpen
    }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                header
                capture
                    .padding(.top, Spacing.s44)
                recent
                    .padding(.top, Spacing.s44)
            }
            .frame(maxWidth: DictateMetrics.columnWidth, alignment: .leading)
            .padding(.top, DictateMetrics.topInset)
            .padding(.horizontal, Spacing.s28)
            .padding(.bottom, Spacing.s44)
            .frame(maxWidth: .infinity)
        }
        .background(Color.minutae.bg)
        .focusable()
        .focusEffectDisabled()
        .focused($isFocused)
        .onKeyPress(.space) {
            Task { await model.toggleDictation() }
            return .handled
        }
        .onAppear { isFocused = true }
        .task { await model.run() }
        .alert("Clear all dictations?", isPresented: $model.isConfirmingClearAll) {
            Button("Clear all", role: .destructive) { model.clearAll() }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(
                "Every dictation and its audio file is removed from this Mac. This can't be undone."
            )
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: Spacing.s4 + Spacing.s2) {
            PageTitle("Just ", emphasis: "say it")
            Text(
                "Speak anywhere, get clean text. Transcribed on this Mac. "
                    + "Every dictation is logged here, with its audio."
            )
            .font(.minutae.ui(size: 13))
            .foregroundStyle(Color.minutae.muted)
            .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var capture: some View {
        VStack(spacing: Spacing.s16 + Spacing.s2) {
            MicButton(
                isListening: model.isListening,
                isBusy: model.isBusy
            ) {
                Task { await model.toggleDictation() }
            }
            Text(hint)
                .font(.minutae.ui(size: 14))
                .foregroundStyle(Color.minutae.ink)
                .multilineTextAlignment(.center)
            HStack(spacing: Spacing.s8) {
                Chip("Pastes into the app you're in", tone: .mint)
                Chip("Audio kept · replay anytime", tone: .lilac)
            }
        }
        .frame(maxWidth: .infinity)
    }

    private var hint: String {
        switch model.phase {
        case .listening: "Listening… press Space to stop"
        case .processing, .inserting: "Transcribing…"
        case .idle, .error: "Hold \(hotkeyLabel) anywhere, or press Space here"
        }
    }

    private var recent: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .firstTextBaseline) {
                SectionLabel("Recent")
                Spacer(minLength: Spacing.s16)
                Button("Clear all") { model.requestClearAll() }
                    .buttonStyle(.plain)
                    .font(.minutae.ui(size: 12, weight: .medium))
                    .foregroundStyle(Color.minutae.muted)
                    .disabled(!model.canClearAll)
                    .opacity(model.canClearAll ? 1 : 0.4)
            }
            .padding(.bottom, Spacing.s8)
            HairlineDivider()
            if let message = model.errorMessage {
                ErrorLine(message: message) { model.dismissError() }
            }
            if model.rows.isEmpty, model.hasLoaded {
                Text("Nothing yet. Each dictation shows up here with its audio.")
                    .font(.minutae.ui(size: 13))
                    .foregroundStyle(Color.minutae.muted)
                    .padding(.vertical, Spacing.s16)
            }
            ForEach(model.rows) { row in
                DictationRow(
                    row: row,
                    isCopied: model.copiedID == row.id,
                    onOpen: { onOpen(row.id) },
                    onCopy: { model.copy(id: row.id) }
                )
                HairlineDivider()
            }
        }
    }
}

enum DictateMetrics {
    static let columnWidth: CGFloat = 720
    static let topInset: CGFloat = 44
}

/// Pine mic in two butter rings. The rings pulse while listening unless Reduce Motion is on.
struct MicButton: View {
    var isListening: Bool
    var isBusy: Bool
    var action: () -> Void
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var pulse = false

    private static let outer: CGFloat = 132
    private static let inner: CGFloat = 96
    private static let core: CGFloat = 76

    var body: some View {
        Button(action: action) {
            ZStack {
                Circle()
                    .fill(Color.minutae.butter.opacity(0.35))
                    .frame(width: Self.outer, height: Self.outer)
                    .scaleEffect(animating ? 1.12 : 1)
                    .opacity(animating ? 0.55 : 1)
                Circle()
                    .fill(Color.minutae.butter.opacity(0.6))
                    .frame(width: Self.inner, height: Self.inner)
                    .scaleEffect(animating ? 1.06 : 1)
                Circle()
                    .fill(Color.minutae.pine)
                    .frame(width: Self.core, height: Self.core)
                Image(systemName: isListening ? "mic.fill" : "mic")
                    .font(.system(size: 28, weight: .light))
                    .foregroundStyle(Color.minutae.white)
                if isListening {
                    Circle()
                        .fill(Color.minutae.live)
                        .frame(width: 10, height: 10)
                        .offset(x: Self.core / 2 - Spacing.s8, y: -(Self.core / 2 - Spacing.s8))
                        .accessibilityHidden(true)
                }
            }
            .frame(width: Self.outer, height: Self.outer)
            .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .disabled(isBusy)
        .accessibilityLabel(isListening ? "Stop dictation" : "Start dictation")
        .accessibilityHint("Or press Space")
        .onChange(of: isListening, initial: true) { _, listening in
            updatePulse(listening)
        }
        .onChange(of: reduceMotion) { _, _ in
            updatePulse(isListening)
        }
    }

    private var animating: Bool { pulse && isListening && !reduceMotion }

    private func updatePulse(_ listening: Bool) {
        guard listening, !reduceMotion else {
            var transaction = Transaction()
            transaction.disablesAnimations = true
            withTransaction(transaction) { pulse = false }
            return
        }
        pulse = false
        withAnimation(.easeInOut(duration: 0.9).repeatForever(autoreverses: true)) {
            pulse = true
        }
    }
}

struct DictationRow: View {
    var row: DictationRowItem
    var isCopied: Bool
    var onOpen: () -> Void
    var onCopy: () -> Void
    @State private var isHovering = false

    var body: some View {
        HStack(alignment: .top, spacing: Spacing.s16) {
            Button(action: onOpen) {
                VStack(alignment: .leading, spacing: Spacing.s4 + Spacing.s2) {
                    Text(row.text)
                        .font(.minutae.ui(size: 16))
                        .lineSpacing(Spacing.s4)
                        .foregroundStyle(
                            row.isPlaceholder ? Color.minutae.muted : Color.minutae.ink
                        )
                        .multilineTextAlignment(.leading)
                        .lineLimit(3)
                        .fixedSize(horizontal: false, vertical: true)
                    Text(row.meta)
                        .font(.minutae.mono(size: 12))
                        .foregroundStyle(Color.minutae.muted)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityHint("Opens the dictation")
            if row.canCopy {
                CopyPill(isCopied: isCopied, action: onCopy)
                    .opacity(isHovering || isCopied ? 1 : 0)
            }
        }
        .padding(.vertical, Spacing.s16)
        .onHover { isHovering = $0 }
        .accessibilityElement(children: .contain)
    }
}

struct CopyPill: View {
    var isCopied: Bool
    var action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: Spacing.s4 + Spacing.s2) {
                Image(systemName: isCopied ? "checkmark" : "doc.on.doc")
                    .font(.minutae.ui(size: 12, weight: .medium))
                    .accessibilityHidden(true)
                Text(isCopied ? "Copied" : "Copy")
                    .font(.minutae.ui(size: 13, weight: .medium))
            }
            .foregroundStyle(Color.minutae.ink)
            .padding(.horizontal, Spacing.s12)
            .frame(height: 32)
            .overlay(Capsule().strokeBorder(Color.minutae.muted.opacity(0.6), lineWidth: 1))
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(isCopied ? "Copied" : "Copy text")
    }
}

struct ErrorLine: View {
    var message: String
    var onDismiss: () -> Void

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
            Text(message)
                .font(.minutae.ui(size: 12))
                .foregroundStyle(Color.minutae.red)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: Spacing.s8)
            Button("Dismiss", action: onDismiss)
                .buttonStyle(.plain)
                .font(.minutae.ui(size: 12, weight: .medium))
                .foregroundStyle(Color.minutae.muted)
        }
        .padding(.vertical, Spacing.s8)
    }
}

#Preview("Dictate") {
    DictateScreen(model: DictatePreview.listModel(), onOpen: { _ in })
        .frame(width: 968, height: 800)
}

#Preview("Dictate · empty") {
    DictateScreen(
        model: DictateViewModel(
            archive: InMemoryDictationArchive(),
            dictation: nil,
            clipboard: PreviewClipboard()
        ),
        onOpen: { _ in }
    )
    .frame(width: 968, height: 800)
}
