import Foundation

enum PackagingProbe {
    static func printNotes() {
        print(
            """
            === Packaging notes (SPIKE-01) ===
            ad_hoc_sign: codesign --force --deep --sign - MinutaeSpike.app
            entitlements: com.apple.security.device.audio-input; Apple Events / Accessibility are TCC, not entitlements
            tcc_persistence: TCC grants bind to the code signature identity + bundle id.
              Ad-hoc (--sign -) identity changes when the CDHash changes across rebuilds unless
              you keep a stable signing identity (Development cert) or use the same Team ID.
              Practical result: ad-hoc rebuilds often re-prompt for Mic / Accessibility / Screen Recording.
            notarization: notarytool submit + staple; requires Apple Developer Program ($99/yr)
            sparkle: Sparkle 2 needs EdDSA key, appcast XML, notarized deltas; works for SwiftUI apps same as AppKit
            deployment_target: macOS 14.4+
            """
        )
    }
}
