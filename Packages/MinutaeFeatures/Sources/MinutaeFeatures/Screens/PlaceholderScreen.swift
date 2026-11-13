import MinutaeDesign
import SwiftUI

/// Stand-in body until a real screen replaces it.
public struct PlaceholderScreen: View {
    private let title: String

    public init(title: String) {
        self.title = title
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s8) {
            Text(title)
                .font(.minutae.display(size: 44))
                .foregroundStyle(Color.minutae.ink)
            Text("This screen is not built yet.")
                .font(.minutae.ui(size: 15))
                .foregroundStyle(Color.minutae.muted)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .padding(Spacing.s28)
        .background(Color.minutae.bg)
        .accessibilityElement(children: .combine)
    }
}
