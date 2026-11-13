import MinutaeDesign
import SwiftUI

/// Everything you have recorded or dictated, in one list. Click a row to open it.
public struct LibraryScreen: View {
    @State private var model: LibraryViewModel?
    private let navigation: NavigationStore?
    private let makeModel: (() -> LibraryViewModel)?

    public init(makeModel: (() -> LibraryViewModel)? = nil, navigation: NavigationStore? = nil) {
        self.makeModel = makeModel
        self.navigation = navigation
    }

    public var body: some View {
        if let model = model ?? makeModel?() {
            LibraryContent(model: model, navigation: navigation)
        } else {
            PlaceholderScreen(title: "Library")
        }
    }
}

struct LibraryContent: View {
    @State var model: LibraryViewModel
    let navigation: NavigationStore?

    init(model: LibraryViewModel, navigation: NavigationStore?) {
        _model = State(initialValue: model)
        self.navigation = navigation
    }

    var body: some View {
        @Bindable var model = model
        PageScaffold("Library") {
            SegmentedPills(
                options: LibraryFilter.allCases.map(\.rawValue),
                selection: Binding(
                    get: { model.filter.rawValue },
                    set: { model.filter = LibraryFilter(rawValue: $0) ?? .all }))
            SearchField(text: $model.query, placeholder: "Filter by title or app")
            if let error = model.error {
                Text(error).font(.minutae.ui(size: 13)).foregroundStyle(Color.minutae.red)
            }
            if model.visible.isEmpty {
                Text(model.items.isEmpty
                    ? "Nothing here yet. Record a meeting or hold ⌥Space to dictate."
                    : "No matches.")
                    .font(.minutae.ui(size: 14))
                    .foregroundStyle(Color.minutae.muted)
                    .padding(.top, Spacing.s12)
            } else {
                VStack(spacing: Spacing.s8) {
                    ForEach(model.visible) { item in
                        LibraryRow(item: item) { navigation?.open(item.target) }
                    }
                }
            }
        }
        .onAppear { model.refresh() }
        .task {
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(3))
                model.refresh()
            }
        }
    }
}

private struct LibraryRow: View {
    let item: LibraryItem
    let action: () -> Void
    @State private var hovering = false

    private var meta: String {
        var parts: [String] = []
        if let date = Formatting.date(item.startedAt) {
            parts.append("\(Formatting.dayHeader(date)) · \(Formatting.time(date))")
        }
        if !item.detail.isEmpty { parts.append(item.detail) }
        return parts.joined(separator: " · ")
    }

    var body: some View {
        Button(action: action) {
            HStack(spacing: Spacing.s12) {
                VStack(alignment: .leading, spacing: Spacing.s4) {
                    Text(item.title)
                        .font(.minutae.ui(size: 15, weight: .medium))
                        .foregroundStyle(Color.minutae.ink)
                        .lineLimit(1)
                    Text(meta)
                    .font(.minutae.ui(size: 12))
                    .foregroundStyle(Color.minutae.muted)
                }
                Spacer(minLength: Spacing.s8)
                Chip(item.kind == .meeting ? "Meeting" : "Dictation",
                    tone: item.kind == .meeting ? .mint : .peach)
            }
            .padding(.horizontal, Spacing.s16).padding(.vertical, Spacing.s12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(hovering ? Color.minutae.raised : Color.minutae.surface,
                in: RoundedRectangle(cornerRadius: Radius.md))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
    }
}
