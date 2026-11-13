import Foundation

/// Default locations under `~/Library/Minutae`, from ARCHITECTURE §1.
public enum MinutaeDirectories {
    public static var root: URL {
        URL(fileURLWithPath: NSHomeDirectory(), isDirectory: true)
            .appending(path: "Library", directoryHint: .isDirectory)
            .appending(path: "Minutae", directoryHint: .isDirectory)
    }

    public static var recordings: URL {
        root.appending(path: "recordings", directoryHint: .isDirectory)
    }

    public static var summaries: URL {
        root.appending(path: "summaries", directoryHint: .isDirectory)
    }

    public static var dictations: URL {
        root.appending(path: "dictations", directoryHint: .isDirectory)
    }
}
