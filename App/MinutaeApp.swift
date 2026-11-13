import MinutaeFeatures
import SwiftUI

@main
struct MinutaeApp: App {
    static let windowTitle = MinutaeFeaturesModule.windowTitle

    var body: some Scene {
        WindowGroup(Self.windowTitle) {
            Color.clear
        }
    }
}
