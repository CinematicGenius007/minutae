import AVFoundation
@preconcurrency import ApplicationServices
import CoreGraphics
import EventKit
import Foundation
import os

/// Live TCC status for microphone, accessibility, screen recording, and calendar.
///
/// `status(of:)` calls only the non-prompting queries. Accessibility and screen
/// recording expose a boolean, so an ungranted value stays `notDetermined` until
/// `request(_:)` has asked once in this process, and is `denied` after that until
/// the live check reports granted.
public final class SystemPermissionService: PermissionService, Sendable {
    private let asked = OSAllocatedUnfairLock(initialState: Set<Permission>())
    private let hub = PermissionChangeHub()

    public init() {}

    public func status(of permission: Permission) -> PermissionStatus {
        switch permission {
        case .microphone:
            Self.map(AVCaptureDevice.authorizationStatus(for: .audio))
        case .calendar:
            Self.map(EKEventStore.authorizationStatus(for: .event))
        case .accessibility:
            booleanStatus(AXIsProcessTrusted(), permission: permission)
        case .screenRecording:
            booleanStatus(CGPreflightScreenCaptureAccess(), permission: permission)
        }
    }

    public func request(_ permission: Permission) async -> PermissionStatus {
        let before = status(of: permission)
        if before == .granted || before == .restricted {
            return refreshed(permission, previous: before)
        }
        // A hard denial is not prompted again. Write-only calendar access is
        // reported as denied because titles need full access, but the system
        // can still upgrade it.
        if before == .denied && !canUpgradeCalendarAccess(permission) {
            return refreshed(permission, previous: before)
        }
        if permission == .accessibility || permission == .screenRecording {
            asked.withLock { _ = $0.insert(permission) }
        }
        await PermissionPrompt.ask(permission)
        return refreshed(permission, previous: before)
    }

    public var changes: AsyncStream<PermissionChange> {
        hub.stream()
    }

    private func canUpgradeCalendarAccess(_ permission: Permission) -> Bool {
        permission == .calendar && EKEventStore.authorizationStatus(for: .event) == .writeOnly
    }

    private func booleanStatus(_ granted: Bool, permission: Permission) -> PermissionStatus {
        if granted {
            return .granted
        }
        let hasAsked = asked.withLock { $0.contains(permission) }
        return hasAsked ? .denied : .notDetermined
    }

    private func refreshed(
        _ permission: Permission,
        previous: PermissionStatus
    ) -> PermissionStatus {
        let after = status(of: permission)
        if after != previous {
            hub.emit(PermissionChange(permission: permission, status: after))
        }
        return after
    }

    private static func map(_ status: AVAuthorizationStatus) -> PermissionStatus {
        switch status {
        case .notDetermined:
            .notDetermined
        case .authorized:
            .granted
        case .denied:
            .denied
        case .restricted:
            .restricted
        @unknown default:
            .denied
        }
    }

    /// Write-only calendar access cannot read event titles, so it is `denied`.
    private static func map(_ status: EKAuthorizationStatus) -> PermissionStatus {
        switch status {
        case .notDetermined:
            .notDetermined
        case .restricted:
            .restricted
        case .denied, .writeOnly:
            .denied
        case .fullAccess:
            .granted
        @unknown default:
            .denied
        }
    }
}

private enum PermissionPrompt {
    static func ask(_ permission: Permission) async {
        switch permission {
        case .microphone:
            await askMicrophone()
        case .accessibility:
            let key = kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String
            await MainActor.run {
                let options = [key: true] as CFDictionary
                _ = AXIsProcessTrustedWithOptions(options)
            }
        case .screenRecording:
            await MainActor.run {
                _ = CGRequestScreenCaptureAccess()
            }
        case .calendar:
            await CalendarAccess().run()
        }
    }

    /// `requestAccess` aborts when the host bundle has no usage string.
    private static func askMicrophone() async {
        guard Bundle.main.object(forInfoDictionaryKey: "NSMicrophoneUsageDescription") != nil else {
            return
        }
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
            AVCaptureDevice.requestAccess(for: .audio) { _ in
                continuation.resume()
            }
        }
    }
}

/// Retains the event store until EventKit calls back. `EKEventStore` is not `Sendable`.
private final class CalendarAccess: @unchecked Sendable {
    private let store = EKEventStore()

    func run() async {
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
            store.requestFullAccessToEvents { _, _ in
                continuation.resume()
            }
        }
    }
}
