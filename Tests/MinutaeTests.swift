import XCTest

final class MinutaeTests: XCTestCase {
    func testBundleIdentifier() {
        XCTAssertEqual(Bundle.main.bundleIdentifier, "com.nebula.minutae")
    }
}
