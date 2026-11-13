import CoreAudio
import Foundation

/// Asks CoreAudio whether any process is using the default input device.
public enum SystemMicProbe {
    public static func isInUse() -> Bool {
        var deviceID = AudioObjectID(kAudioObjectUnknown)
        var size = UInt32(MemoryLayout<AudioObjectID>.size)
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDefaultInputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain)
        guard
            AudioObjectGetPropertyData(
                AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &deviceID)
                == noErr, deviceID != kAudioObjectUnknown
        else { return false }
        var running: UInt32 = 0
        size = UInt32(MemoryLayout<UInt32>.size)
        address.mSelector = kAudioDevicePropertyDeviceIsRunningSomewhere
        guard AudioObjectGetPropertyData(deviceID, &address, 0, nil, &size, &running) == noErr
        else { return false }
        return running != 0
    }
}

/// Turns raw "mic is running" samples into one "a call probably started" event.
///
/// The mic must stay in use for `sustain` consecutive samples while Minutae is not capturing
/// itself, so a short dictation or a system sound does not trigger a prompt. After a prompt the
/// detector stays quiet until the mic has been idle again.
public struct MicActivityDetector: Sendable {
    public let sustain: Int
    private var streak = 0
    private var armed = true

    public init(sustain: Int = 3) { self.sustain = max(sustain, 1) }

    /// Returns true on the sample that should raise a prompt.
    public mutating func ingest(micInUse: Bool, ownCapture: Bool) -> Bool {
        guard micInUse, !ownCapture else {
            streak = 0
            if !micInUse { armed = true }
            return false
        }
        streak += 1
        guard armed, streak >= sustain else { return false }
        armed = false
        return true
    }

    /// Call when the user records or dismisses so the same call is not offered twice.
    public mutating func disarm() { armed = false }
}
