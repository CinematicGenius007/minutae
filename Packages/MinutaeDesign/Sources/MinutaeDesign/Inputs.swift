import SwiftUI

/// One choice in a `Dropdown`.
public struct DropdownOption<ID: Hashable>: Identifiable {
    public let id: ID
    public let title: String
    public let detail: String?

    public init(_ id: ID, title: String, detail: String? = nil) {
        self.id = id
        self.title = title
        self.detail = detail
    }
}

/// Custom select. A pill trigger that opens a list of rows on a light card.
/// Arrow keys move, Return picks, Escape closes.
public struct Dropdown<ID: Hashable>: View {
    @Binding private var selection: ID
    private let options: [DropdownOption<ID>]
    private let label: String
    private let width: CGFloat
    @State private var isOpen = false
    @State private var hovering = false

    public init(
        _ label: String,
        selection: Binding<ID>,
        options: [DropdownOption<ID>],
        width: CGFloat = 200
    ) {
        self.label = label
        _selection = selection
        self.options = options
        self.width = width
    }

    private var current: DropdownOption<ID>? { options.first { $0.id == selection } }

    public var body: some View {
        Button {
            isOpen.toggle()
        } label: {
            HStack(spacing: Spacing.s8) {
                Text(current?.title ?? "Choose")
                    .font(.minutae.ui(size: 14))
                    .foregroundStyle(Color.minutae.ink)
                    .lineLimit(1)
                    .truncationMode(.tail)
                Spacer(minLength: Spacing.s4)
                Image(systemName: "chevron.down")
                    .font(.minutae.ui(size: 11, weight: .semibold))
                    .foregroundStyle(Color.minutae.muted)
                    .rotationEffect(.degrees(isOpen ? 180 : 0))
                    .accessibilityHidden(true)
            }
            .padding(.horizontal, Spacing.s12)
            .frame(width: width, height: 36)
            .background(hovering || isOpen ? Color.minutae.raised : Color.minutae.surface, in: Capsule())
            .overlay(Capsule().strokeBorder(isOpen ? Color.minutae.pine : Color.minutae.line, lineWidth: 1))
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
        .accessibilityLabel(label)
        .accessibilityValue(current?.title ?? "")
        .popover(isPresented: $isOpen, arrowEdge: .bottom) {
            DropdownList(options: options, selection: $selection, width: max(width, 220)) {
                isOpen = false
            }
        }
    }
}

private struct DropdownList<ID: Hashable>: View {
    let options: [DropdownOption<ID>]
    @Binding var selection: ID
    let width: CGFloat
    let close: () -> Void
    @State private var highlighted: ID?
    @FocusState private var focused: Bool

    var body: some View {
        VStack(spacing: Spacing.s2) {
            ForEach(options) { option in
                row(option)
            }
        }
        .padding(Spacing.s4)
        .frame(width: width)
        .background(Color.minutae.bg)
        .focusable()
        .focusEffectDisabled()
        .focused($focused)
        .onAppear {
            highlighted = selection
            focused = true
        }
        .onKeyPress(.downArrow) { move(1); return .handled }
        .onKeyPress(.upArrow) { move(-1); return .handled }
        .onKeyPress(.return) {
            if let highlighted { selection = highlighted }
            close()
            return .handled
        }
        .onKeyPress(.escape) { close(); return .handled }
        .preferredColorScheme(.light)
    }

    private func move(_ delta: Int) {
        guard let index = options.firstIndex(where: { $0.id == highlighted }) else {
            highlighted = options.first?.id
            return
        }
        let next = min(max(index + delta, 0), options.count - 1)
        highlighted = options[next].id
    }

    private func row(_ option: DropdownOption<ID>) -> some View {
        let isSelected = option.id == selection
        let isHighlighted = option.id == highlighted
        return Button {
            selection = option.id
            close()
        } label: {
            HStack(spacing: Spacing.s8) {
                VStack(alignment: .leading, spacing: Spacing.s2) {
                    Text(option.title)
                        .font(.minutae.ui(size: 14, weight: isSelected ? .medium : .regular))
                        .foregroundStyle(Color.minutae.ink)
                    if let detail = option.detail {
                        Text(detail)
                            .font(.minutae.ui(size: 12))
                            .foregroundStyle(Color.minutae.muted)
                    }
                }
                Spacer(minLength: Spacing.s8)
                if isSelected {
                    Image(systemName: "checkmark")
                        .font(.minutae.ui(size: 12, weight: .semibold))
                        .foregroundStyle(Color.minutae.pine)
                        .accessibilityHidden(true)
                }
            }
            .padding(.horizontal, Spacing.s12)
            .padding(.vertical, Spacing.s8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                isHighlighted ? Color.minutae.surface : Color.clear,
                in: RoundedRectangle(cornerRadius: Radius.sm)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { if $0 { highlighted = option.id } }
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}

/// Single-line text input on a pill. `secure` hides the characters.
public struct MinutaeField: View {
    @Binding private var text: String
    private let placeholder: String
    private let secure: Bool
    private let width: CGFloat?
    @FocusState private var focused: Bool

    public init(
        _ placeholder: String,
        text: Binding<String>,
        secure: Bool = false,
        width: CGFloat? = nil
    ) {
        self.placeholder = placeholder
        _text = text
        self.secure = secure
        self.width = width
    }

    public var body: some View {
        ZStack(alignment: .leading) {
            if text.isEmpty {
                Text(placeholder)
                    .font(.minutae.ui(size: 14))
                    .foregroundStyle(Color.minutae.muted)
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
            }
            Group {
                if secure {
                    SecureField("", text: $text)
                } else {
                    TextField("", text: $text)
                }
            }
            .textFieldStyle(.plain)
            .font(.minutae.ui(size: 14))
            .foregroundStyle(Color.minutae.ink)
            .focused($focused)
            .accessibilityLabel(placeholder)
        }
        .padding(.horizontal, Spacing.s12)
        .frame(width: width, height: 36)
        .frame(maxWidth: width == nil ? .infinity : nil)
        .background(Color.minutae.surface, in: Capsule())
        .overlay(
            Capsule().strokeBorder(focused ? Color.minutae.pine : Color.minutae.line, lineWidth: focused ? 1.5 : 1)
        )
        .contentShape(Capsule())
        .onTapGesture { focused = true }
    }
}

/// Small pill-shaped text action with hover, for trailing row actions.
public struct TextAction: View {
    public enum Tone: Sendable { case normal, destructive }
    private let title: String
    private let tone: Tone
    private let action: () -> Void
    @State private var hovering = false

    public init(_ title: String, tone: Tone = .normal, action: @escaping () -> Void) {
        self.title = title
        self.tone = tone
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            Text(title)
                .font(.minutae.ui(size: 13, weight: .medium))
                .foregroundStyle(tone == .destructive ? Color.minutae.red : Color.minutae.pine)
                .padding(.horizontal, Spacing.s12)
                .padding(.vertical, Spacing.s8)
                .background(hovering ? Color.minutae.surface : Color.clear, in: Capsule())
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
    }
}

/// Centres page content at a readable width and scrolls when the window is short.
public struct PageScaffold<Content: View>: View {
    private let title: String
    private let maxWidth: CGFloat
    private let spacing: CGFloat
    private let content: Content

    public init(
        _ title: String, maxWidth: CGFloat = 720, spacing: CGFloat = Spacing.s20,
        @ViewBuilder content: () -> Content
    ) {
        self.title = title
        self.maxWidth = maxWidth
        self.spacing = spacing
        self.content = content()
    }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: spacing) {
                Text(title)
                    .font(.minutae.display(size: 36))
                    .foregroundStyle(Color.minutae.ink)
                    .accessibilityAddTraits(.isHeader)
                    .padding(.bottom, Spacing.s20)
                content
            }
            .frame(maxWidth: maxWidth, alignment: .leading)
            .padding(.horizontal, Spacing.s28)
            .padding(.top, Spacing.s44)
            .padding(.bottom, Spacing.s44)
            .frame(maxWidth: .infinity)
        }
        .foregroundStyle(Color.minutae.ink)
        .background(Color.minutae.bg)
    }
}
