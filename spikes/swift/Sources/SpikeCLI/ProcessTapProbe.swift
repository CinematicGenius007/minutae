import AppKit
import AudioToolbox
import CoreAudio
import Foundation

enum ProcessTapProbe {
    static func run(args: [String]) throws {
        if args.contains("--list") {
            try listProcesses()
            return
        }

        var pid: pid_t?
        if let idx = args.firstIndex(of: "--pid"), args.indices.contains(idx + 1) {
            pid = pid_t(args[idx + 1])
        }

        print("=== Core Audio process tap ===")
        print("os: \(ProcessInfo.processInfo.operatingSystemVersionString)")
        print("api: AudioHardwareCreateProcessTap / CATapDescription (macOS 14.2+)")

        guard #available(macOS 14.2, *) else {
            print("result: fails")
            print("why: process taps require macOS 14.2+")
            return
        }

        let targetPID: pid_t
        if let pid {
            targetPID = pid
        } else if let auto = findInterestingPID() {
            targetPID = auto
            print("auto_pid: \(targetPID)")
        } else {
            print("result: partly works")
            print("why: API present, but no Zoom/Chrome/Music/Slack process found to tap. Pass --pid.")
            try listProcesses()
            return
        }

        try tapProcess(pid: targetPID)
    }

    @available(macOS 14.2, *)
    private static func tapProcess(pid: pid_t) throws {
        // CATapDescription init with process object IDs
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyTranslatePIDToProcessObject,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var processObjectID = AudioObjectID(kAudioObjectUnknown)
        var pidCopy = pid
        var size = UInt32(MemoryLayout<AudioObjectID>.size)
        let status = withUnsafeMutablePointer(to: &pidCopy) { pidPtr in
            AudioObjectGetPropertyData(
                AudioObjectID(kAudioObjectSystemObject),
                &address,
                UInt32(MemoryLayout<pid_t>.size),
                pidPtr,
                &size,
                &processObjectID
            )
        }
        if status != noErr {
            print("result: fails")
            print("translate_pid_status: \(status)")
            print("why: could not map pid \(pid) to an audio process object (app may not be playing audio)")
            print("permission_note: System Settings → Privacy & Security → Screen Recording (and Microphone on some builds) are typically required for process taps. Denial yields no samples / property errors, not a crash.")
            return
        }
        print("process_object_id: \(processObjectID)")

        let description = CATapDescription(stereoMixdownOfProcesses: [processObjectID])
        description.name = "MinutaeSpikeTap"
        description.isPrivate = true
        description.muteBehavior = .unmuted

        var tapID = AudioObjectID(kAudioObjectUnknown)
        let createStatus = AudioHardwareCreateProcessTap(description, &tapID)
        print("create_status: \(createStatus)")
        if createStatus != noErr {
            print("result: fails")
            print("why: AudioHardwareCreateProcessTap returned \(createStatus). On macOS 14.4 this usually means missing Screen Recording permission or an invalid process object.")
            print("permission_denied_behaviour: no Them stream; user must grant Screen Recording and relaunch. TCC does not throw a Swift error — check OSStatus.")
            return
        }
        print("tap_id: \(tapID)")
        print("result: works")
        print("detail: process tap created for pid \(pid). Destroying immediately (spike only).")
        print("permission_note: Screen Recording prompted/required on first use on recent macOS. Microphone alone is not enough for other apps' audio.")
        AudioHardwareDestroyProcessTap(tapID)
    }

    private static func findInterestingPID() -> pid_t? {
        let names = ["zoom.us", "Google Chrome", "Microsoft Edge", "Music", "Slack", "firefox"]
        let workspace = NSWorkspace.shared
        for app in workspace.runningApplications {
            if let name = app.localizedName, names.contains(name) {
                return app.processIdentifier
            }
            if let bundle = app.bundleIdentifier {
                if bundle.contains("zoom") || bundle.contains("chrome") || bundle.contains("slack") {
                    return app.processIdentifier
                }
            }
        }
        return workspace.frontmostApplication?.processIdentifier
    }

    private static func listProcesses() throws {
        print("running_audio_candidates:")
        for app in NSWorkspace.shared.runningApplications.prefix(40) {
            let name = app.localizedName ?? "?"
            print("  pid=\(app.processIdentifier) name=\(name) bundle=\(app.bundleIdentifier ?? "-")")
        }
    }
}
