import CoreText
import Foundation

/// What `MinutaeDesign.registerFonts()` managed to load.
public struct FontRegistrationResult: Equatable, Sendable {
    public var registered: [String]
    public var missing: [String]
    public var failed: [String]
    public var displayFamily: String?
    public var uiFamily: String?
    public var monoFamily: String?
    /// True when the Fraunces italic file registered and a family name was read.
    public var italicReady: Bool
}

enum FontRole: String, CaseIterable, Sendable {
    case display
    case displayItalic
    case sans
    case mono

    var filename: String {
        switch self {
        case .display:
            "Fraunces-Variable"
        case .displayItalic:
            "Fraunces-Italic-Variable"
        case .sans:
            "Geist-Variable"
        case .mono:
            "GeistMono-Variable"
        }
    }

    var expectsItalicStyle: Bool { self == .displayItalic }
}

struct InstalledFaces: Sendable {
    var displayFamily: String?
    var italicFamily: String?
    var italicNeedsTrait: Bool
    var uiFamily: String?
    var monoFamily: String?

    static let empty = InstalledFaces(
        displayFamily: nil,
        italicFamily: nil,
        italicNeedsTrait: false,
        uiFamily: nil,
        monoFamily: nil
    )
}

/// Process-wide record of which bundled families are usable.
final class FontBook: @unchecked Sendable {
    static let shared = FontBook()

    private let lock = NSLock()
    private var didRegister = false
    private var faces = InstalledFaces.empty
    private var lastResult = FontRegistrationResult(
        registered: [],
        missing: [],
        failed: [],
        displayFamily: nil,
        uiFamily: nil,
        monoFamily: nil,
        italicReady: false
    )

    func registerBundle() -> FontRegistrationResult {
        lock.lock()
        defer { lock.unlock() }
        if didRegister { return lastResult }
        let outcome = FontRegistrar.registerBundled(Self.bundledFontURLs())
        faces = outcome.faces
        lastResult = outcome.result
        didRegister = true
        return lastResult
    }

    func snapshot() -> InstalledFaces {
        _ = registerBundle()
        lock.lock()
        defer { lock.unlock() }
        return faces
    }

    static func bundledFontURLs() -> [FontRole: URL] {
        var found: [FontRole: URL] = [:]
        let files = bundledFiles { $0.pathExtension.lowercased() == "ttf" }
        for url in files {
            let stem = url.deletingPathExtension().lastPathComponent
            guard let role = FontRole.allCases.first(where: { $0.filename == stem }) else {
                continue
            }
            found[role] = url
        }
        return found
    }

    static func licenseURLs() -> [URL] {
        bundledFiles { $0.lastPathComponent.hasSuffix("OFL.txt") }
    }

    private static func bundledFiles(where include: (URL) -> Bool) -> [URL] {
        var matches: [URL] = []
        var seen = Set<String>()
        for root in bundleRoots() {
            guard
                let enumerator = FileManager.default.enumerator(
                    at: root,
                    includingPropertiesForKeys: [.isRegularFileKey],
                    options: [.skipsHiddenFiles]
                )
            else { continue }
            for case let url as URL in enumerator {
                guard include(url), seen.insert(url.lastPathComponent).inserted else { continue }
                matches.append(url)
            }
        }
        return matches
    }

    private static func bundleRoots() -> [URL] {
        var roots: [URL] = []
        if let resource = Bundle.module.resourceURL {
            roots.append(resource)
        }
        let bundle = Bundle.module.bundleURL
        if !roots.contains(bundle) {
            roots.append(bundle)
        }
        return roots
    }
}

enum FontRegistrar {
    struct BundleOutcome: Sendable {
        var result: FontRegistrationResult
        var faces: InstalledFaces
    }

    /// Registers whatever URLs it is given. Missing and invalid files are recorded, never thrown.
    static func register(urls: [URL]) -> FontRegistrationResult {
        var registered: [String] = []
        var missing: [String] = []
        var failed: [String] = []
        for url in urls {
            switch classify(url) {
            case .missing:
                missing.append(url.lastPathComponent)
            case .registered:
                registered.append(url.lastPathComponent)
            case .failed:
                failed.append(url.lastPathComponent)
            }
        }
        return FontRegistrationResult(
            registered: registered,
            missing: missing,
            failed: failed,
            displayFamily: nil,
            uiFamily: nil,
            monoFamily: nil,
            italicReady: false
        )
    }

    static func registerBundled(_ urls: [FontRole: URL]) -> BundleOutcome {
        var registered: [String] = []
        var missing: [String] = []
        var failed: [String] = []
        var families: [FontRole: String] = [:]

        for role in FontRole.allCases {
            guard let url = urls[role] else {
                missing.append("\(role.filename).ttf")
                continue
            }
            let name = url.lastPathComponent
            switch classify(url) {
            case .missing:
                missing.append(name)
            case .failed:
                failed.append(name)
            case .registered:
                registered.append(name)
                if let family = preferredFamily(at: url, italic: role.expectsItalicStyle) {
                    families[role] = family
                }
            }
        }

        let display = families[.display]
        let italic = families[.displayItalic]
        let faces = InstalledFaces(
            displayFamily: display,
            italicFamily: italic,
            italicNeedsTrait: italic != nil && italic == display,
            uiFamily: families[.sans],
            monoFamily: families[.mono]
        )
        let result = FontRegistrationResult(
            registered: registered,
            missing: missing,
            failed: failed,
            displayFamily: display,
            uiFamily: families[.sans],
            monoFamily: families[.mono],
            italicReady: italic != nil
        )
        return BundleOutcome(result: result, faces: faces)
    }

    private enum FileStatus {
        case missing
        case registered
        case failed
    }

    private static func classify(_ url: URL) -> FileStatus {
        var isDirectory: ObjCBool = false
        let exists = FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory)
        if !exists || isDirectory.boolValue { return .missing }
        var unmanaged: Unmanaged<CFError>?
        let didRegister = CTFontManagerRegisterFontsForURL(url as CFURL, .process, &unmanaged)
        let code = unmanaged.map { CFErrorGetCode($0.takeRetainedValue()) }
        if didRegister || code == CTFontManagerError.alreadyRegistered.rawValue {
            return .registered
        }
        return .failed
    }

    private static func preferredFamily(at url: URL, italic: Bool) -> String? {
        let faces = faceNames(at: url)
        if italic {
            if let exact = faces.first(where: {
                $0.style.compare("Italic", options: .caseInsensitive) == .orderedSame
            }) {
                return exact.family
            }
            let partial = faces.first {
                $0.style.range(of: "italic", options: .caseInsensitive) != nil
            }
            return partial?.family ?? faces.first?.family
        }
        let regular = faces.first {
            $0.style.compare("Regular", options: .caseInsensitive) == .orderedSame
        }
        return regular?.family ?? faces.first?.family
    }

    private static func faceNames(at url: URL) -> [(family: String, style: String)] {
        guard
            let listed = CTFontManagerCreateFontDescriptorsFromURL(url as CFURL)
                as? [CTFontDescriptor]
        else {
            return []
        }
        var faces: [(family: String, style: String)] = []
        for descriptor in listed {
            let family =
                CTFontDescriptorCopyAttribute(descriptor, kCTFontFamilyNameAttribute) as? String
            let style =
                CTFontDescriptorCopyAttribute(descriptor, kCTFontStyleNameAttribute) as? String
            if let family, !family.isEmpty {
                faces.append((family, style ?? ""))
            }
        }
        return faces
    }
}
