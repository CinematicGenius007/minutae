import MinutaeDesign
import SwiftUI

/// Dictate and Dictation detail in one view, so the shell registers a single destination:
///
///     DictateRootView(dependencies: .live(database:, dictationsDirectory:, dictation:))
public struct DictateRootView: View {
    private let dependencies: DictateDependencies
    @State private var listModel: DictateViewModel
    @State private var openID: String?

    public init(dependencies: DictateDependencies, openID: String? = nil) {
        self.dependencies = dependencies
        _listModel = State(initialValue: DictateViewModel(dependencies: dependencies))
        _openID = State(initialValue: openID)
    }

    public var body: some View {
        Group {
            if let openID {
                DictationDetailContainer(id: openID, dependencies: dependencies) {
                    self.openID = nil
                }
                .id(openID)
            } else {
                DictateScreen(model: listModel, hotkeyLabel: dependencies.hotkeyLabel) { id in
                    openID = id
                }
            }
        }
        .background(Color.minutae.bg)
    }
}

/// Owns one detail view model for the life of one opened dictation.
struct DictationDetailContainer: View {
    @State private var model: DictationDetailViewModel
    private let onBack: () -> Void

    init(id: String, dependencies: DictateDependencies, onBack: @escaping () -> Void) {
        _model = State(initialValue: DictationDetailViewModel(id: id, dependencies: dependencies))
        self.onBack = onBack
    }

    var body: some View {
        DictationDetailScreen(model: model, onBack: onBack)
    }
}

#Preview("Dictate root") {
    DictateRootView(dependencies: DictatePreview.dependencies())
        .frame(width: 968, height: 800)
}
