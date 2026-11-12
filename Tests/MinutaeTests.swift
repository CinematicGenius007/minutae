import XCTest
@testable import Minutae

final class MinutaeTests: XCTestCase {
    @MainActor func testFeatureWindowTitle() {
        let title = MinutaeApp.windowTitle
        XCTAssertEqual(title, "Minutae")
    }

    func testBundleIdentifier() {
        XCTAssertEqual(Bundle.main.bundleIdentifier, "com.nebula.minutae")
    }
}
