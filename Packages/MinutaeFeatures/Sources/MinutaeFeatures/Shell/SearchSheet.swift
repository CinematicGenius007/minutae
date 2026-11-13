import MinutaeDesign
import SwiftUI

/// Placeholder for the ⌘K search sheet.
public struct SearchSheet: View {
    public init() {}

    public var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s12) {
            Text("Search")
                .font(.minutae.display(size: 32))
                .foregroundStyle(Color.minutae.ink)
            Text("Search is not available yet.")
                .font(.minutae.ui(size: 15))
                .foregroundStyle(Color.minutae.muted)
        }
        .padding(Spacing.s28)
        .frame(width: 520, alignment: .leading)
        .frame(minHeight: 180, alignment: .topLeading)
        .background(Color.minutae.bg)
        .preferredColorScheme(.light)
    }
}
