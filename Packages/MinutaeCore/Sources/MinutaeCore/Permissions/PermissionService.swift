/// Status, just-in-time requests, and the denial fallback for each permission.
///
/// `status(of:)` must never present a system prompt. `request(_:)` asks only when
/// the status is still `notDetermined`. The plain-language reason for that ask is
/// `Permission.reason`.
public protocol PermissionService: Sendable {
    func status(of permission: Permission) -> PermissionStatus

    func request(_ permission: Permission) async -> PermissionStatus

    /// A new stream of later status changes. Each access subscribes independently
    /// and does not replay the current status.
    var changes: AsyncStream<PermissionChange> { get }

    func fallback(for permission: Permission) -> PermissionFallback
}

extension PermissionService {
    public func fallback(for permission: Permission) -> PermissionFallback {
        PermissionFallback.defined(for: permission)
    }
}
