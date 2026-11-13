import MinutaeDesign
import MinutaeFeatures
import SwiftUI

@main
struct MinutaeApp: App {
    static let windowTitle = MinutaeFeaturesModule.windowTitle
    @State private var model: DebugHarnessModel

    init() {
        MinutaeDesign.registerFonts()
        _model = State(initialValue: DebugHarnessModel())
    }

    var body: some Scene {
        WindowGroup(Self.windowTitle) {
            DebugHarnessView(model: model)
        }
    }
}
