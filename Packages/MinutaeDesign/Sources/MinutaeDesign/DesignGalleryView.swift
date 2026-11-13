import SwiftUI

/// Every token and control on the 1200×800 Ink & signal artboard.
public struct DesignGalleryView: View {
    public init() {}

    public var body: some View {
        HStack(spacing: 0) {
            GallerySidebar()
            ScrollView {
                VStack(alignment: .leading, spacing: Spacing.s28) {
                    PageTitle("Just ", emphasis: "say", trailing: " it")
                    GalleryColors()
                    GalleryType()
                    GalleryMetrics()
                    GalleryButtons()
                    GalleryFields()
                    GallerySettings()
                    GalleryCollection()
                    GalleryHUDSection()
                    GalleryEmpty()
                }
                .padding(Spacing.s28)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .scrollContentBackground(.hidden)
            .background(Color.minutae.bg)
        }
        .frame(width: Layout.windowWidth, height: Layout.windowHeight)
        .background(Color.minutae.bg)
        .preferredColorScheme(.light)
        .onAppear { _ = FontBook.shared.registerBundle() }
    }
}

private struct GallerySidebar: View {
    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s8) {
            Text("Minutae")
                .font(.minutae.display(size: 28))
                .foregroundStyle(Color.minutae.ink)
                .padding(.bottom, Spacing.s8)
            SectionLabel("Library")
            SidebarItem("Meetings", systemImage: "calendar", isSelected: true) {}
            SidebarItem("Dictate", systemImage: "waveform", isSelected: false) {}
            SidebarItem("Search", systemImage: "magnifyingglass", isSelected: false) {}
            SidebarItem("Settings", systemImage: "gearshape", isSelected: false) {}
            Spacer(minLength: Spacing.s16)
            StatusPill("Listening for meetings", tone: .mint)
        }
        .padding(Spacing.s16)
        .frame(width: Layout.sidebarWidth)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(Color.minutae.surface)
    }
}

#Preview("Design gallery") {
    DesignGalleryView()
}
