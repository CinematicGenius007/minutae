import MinutaeDesign
import MinutaeLibrary
import SwiftUI

/// ⌘K sheet: type to search meetings and dictations. Up/Down choose, Return opens, Esc closes.
public struct SearchSheet: View {
    private let model: SearchModel?
    private let onOpen: (SearchHit) -> Void
    private let onClose: () -> Void
    @FocusState private var fieldFocused: Bool

    public init(
        model: SearchModel? = nil,
        onOpen: @escaping (SearchHit) -> Void = { _ in },
        onClose: @escaping () -> Void = {}
    ) {
        self.model = model
        self.onOpen = onOpen
        self.onClose = onClose
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let model {
                content(model)
            } else {
                Text("Search is not available right now.")
                    .font(.minutae.ui(size: 14))
                    .foregroundStyle(Color.minutae.muted)
                    .padding(Spacing.s28)
            }
        }
        .frame(width: 600)
        .frame(minHeight: 140, maxHeight: 520, alignment: .top)
        .background(Color.minutae.bg)
        .preferredColorScheme(.light)
        .onKeyPress(.escape) {
            onClose()
            return .handled
        }
    }

    @ViewBuilder private func content(_ model: SearchModel) -> some View {
        @Bindable var model = model
        HStack(spacing: Spacing.s12) {
            Image(systemName: "magnifyingglass")
                .font(.minutae.ui(size: 16, weight: .medium))
                .foregroundStyle(Color.minutae.muted)
                .accessibilityHidden(true)
            TextField("Search meetings and dictations", text: $model.query)
                .textFieldStyle(.plain)
                .font(.minutae.ui(size: 18))
                .foregroundStyle(Color.minutae.ink)
                .focused($fieldFocused)
                .onSubmit { if let hit = model.selectedHit { onOpen(hit) } }
            KeyCap("esc", accessibilityText: "Escape")
        }
        .padding(.horizontal, Spacing.s20)
        .frame(height: 60)
        .onKeyPress(.downArrow) { model.move(1); return .handled }
        .onKeyPress(.upArrow) { model.move(-1); return .handled }
        HairlineDivider()
        results(model)
            .task {
                fieldFocused = true
                await model.prepare()
            }
    }

    @ViewBuilder private func results(_ model: SearchModel) -> some View {
        let trimmed = model.query.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty {
            hint("Type to search transcripts, summaries and dictations.")
        } else if let error = model.error {
            hint(error)
        } else if model.hits.isEmpty {
            hint(model.isSearching ? "Searching…" : "No matches for “\(trimmed)”.")
        } else {
            ScrollViewReader { proxy in
                ScrollView {
                    VStack(spacing: Spacing.s2) {
                        ForEach(Array(model.hits.enumerated()), id: \.offset) { index, hit in
                            SearchRow(hit: hit, isSelected: index == model.selection) {
                                onOpen(hit)
                            }
                            .id(index)
                            .onHover { if $0 { model.selection = index } }
                        }
                    }
                    .padding(Spacing.s8)
                }
                .onChange(of: model.selection) { _, new in proxy.scrollTo(new) }
            }
        }
    }

    private func hint(_ text: String) -> some View {
        Text(text)
            .font(.minutae.ui(size: 14))
            .foregroundStyle(Color.minutae.muted)
            .padding(Spacing.s20)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private struct SearchRow: View {
    let hit: SearchHit
    let isSelected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: Spacing.s4) {
                HStack(spacing: Spacing.s8) {
                    Text(hit.title.isEmpty ? "Untitled" : hit.title)
                        .font(.minutae.ui(size: 15, weight: .medium))
                        .foregroundStyle(Color.minutae.ink)
                        .lineLimit(1)
                    Chip(hit.kind == .meeting ? "Meeting" : "Dictation",
                        tone: hit.kind == .meeting ? .mint : .peach)
                    Spacer(minLength: Spacing.s8)
                    if let date = Formatting.date(hit.date) {
                        Text(date.formatted(.dateTime.day().month(.abbreviated)))
                            .font(.minutae.ui(size: 12))
                            .foregroundStyle(Color.minutae.muted)
                    }
                }
                if !hit.snippet.isEmpty {
                    Text(hit.snippet)
                        .font(.minutae.ui(size: 13))
                        .foregroundStyle(Color.minutae.muted)
                        .lineLimit(2)
                        .multilineTextAlignment(.leading)
                }
            }
            .padding(.horizontal, Spacing.s12).padding(.vertical, Spacing.s8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                isSelected ? Color.minutae.surface : Color.clear,
                in: RoundedRectangle(cornerRadius: Radius.sm)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}
