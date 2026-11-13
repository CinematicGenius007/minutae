import MinutaeDesign
import SwiftUI

/// Light app shell: 232 pt sidebar and the screen for the selected route.
public struct AppShellView: View {
    @Bindable private var navigation: NavigationStore
    private let screens: ScreenRegistry
    private let isRecording: Bool
    private let canRecord: Bool
    private let actionMessage: String?
    private let bootError: String?
    private let onToggleRecording: () -> Void
    private let onSearch: () -> Void
    @Binding private var isSearchPresented: Bool
    /// Bumps when a route's model is replaced so the screen's `@State` takes the new value.
    private let screenEpoch: Int

    public init(
        navigation: NavigationStore,
        screens: ScreenRegistry = .placeholders,
        isRecording: Bool,
        canRecord: Bool,
        actionMessage: String? = nil,
        bootError: String? = nil,
        onToggleRecording: @escaping () -> Void,
        onSearch: @escaping () -> Void,
        isSearchPresented: Binding<Bool>,
        screenEpoch: Int = 0
    ) {
        self.navigation = navigation
        self.screens = screens
        self.isRecording = isRecording
        self.canRecord = canRecord
        self.actionMessage = actionMessage
        self.bootError = bootError
        self.onToggleRecording = onToggleRecording
        self.onSearch = onSearch
        _isSearchPresented = isSearchPresented
        self.screenEpoch = screenEpoch
    }

    public var body: some View {
        HStack(spacing: 0) {
            sidebar
            detail
        }
        .frame(
            minWidth: Layout.sidebarWidth + 520,
            idealWidth: Layout.windowWidth,
            minHeight: 640,
            idealHeight: Layout.windowHeight
        )
        .background {
            HStack(spacing: 0) {
                Color.minutae.surface.frame(width: Layout.sidebarWidth)
                Color.minutae.bg
            }
            .ignoresSafeArea()
        }
        .preferredColorScheme(.light)
        .sheet(isPresented: $isSearchPresented) {
            SearchSheet()
        }
    }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: Spacing.s8) {
            Text("Minutae")
                .font(.minutae.display(size: 28))
                .foregroundStyle(Color.minutae.ink)
                .accessibilityAddTraits(.isHeader)
                .padding(.bottom, Spacing.s8)
            RecordButton(
                "Record",
                shortcut: "⌘⇧R",
                isCapturing: isRecording,
                action: onToggleRecording
            )
            .disabled(!canRecord)
            SearchPill(action: onSearch)
            VStack(spacing: Spacing.s4) {
                ForEach(Route.allCases) { route in
                    SidebarItem(
                        route.title,
                        systemImage: route.systemImage,
                        isSelected: navigation.route == route
                    ) {
                        navigation.select(route)
                    }
                }
            }
            .padding(.top, Spacing.s8)
            if let actionMessage, !actionMessage.isEmpty {
                Text(actionMessage)
                    .font(.minutae.ui(size: 12))
                    .foregroundStyle(Color.minutae.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: Spacing.s16)
            StatusPill(Self.listeningTitle, tone: .mint)
        }
        .padding(Spacing.s16)
        .frame(width: Layout.sidebarWidth)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(Color.minutae.surface)
    }

    private var detail: some View {
        VStack(alignment: .leading, spacing: Spacing.s12) {
            if let bootError, !bootError.isEmpty {
                Text(bootError)
                    .font(.minutae.ui(size: 14))
                    .foregroundStyle(Color.minutae.red)
                    .padding(.horizontal, Spacing.s28)
                    .padding(.top, Spacing.s16)
            }
            screens.view(for: navigation.route)
                .id(screenEpoch)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(Color.minutae.bg)
    }

    /// No live meeting-detection value exists on the composition root yet.
    static let listeningTitle = "Listening for meetings"
}

private struct SearchPill: View {
    var action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: Spacing.s8) {
                Image(systemName: "magnifyingglass")
                    .font(.minutae.ui(size: 14, weight: .medium))
                    .foregroundStyle(Color.minutae.muted)
                    .accessibilityHidden(true)
                Text("Search")
                    .font(.minutae.ui(size: 14))
                    .foregroundStyle(Color.minutae.muted)
                Spacer(minLength: Spacing.s8)
                KeyCap("⌘K", accessibilityText: "Command K")
            }
            .padding(.horizontal, Spacing.s12)
            .padding(.vertical, Spacing.s8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.minutae.bg, in: Capsule())
            .overlay(Capsule().strokeBorder(Color.minutae.line, lineWidth: 1))
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Search")
        .accessibilityHint("Command K")
    }
}

#Preview("App shell") {
    AppShellView(
        navigation: NavigationStore(),
        isRecording: false,
        canRecord: true,
        onToggleRecording: {},
        onSearch: {},
        isSearchPresented: .constant(false)
    )
}
