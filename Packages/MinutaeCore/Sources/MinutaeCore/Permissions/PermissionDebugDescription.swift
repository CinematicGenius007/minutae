/// One log line for a permission. Not user-interface copy.
public enum PermissionDebugDescription {
    public static func line(permission: Permission, status: PermissionStatus) -> String {
        let fallback = PermissionFallback.defined(for: permission)
        return """
            \(permission.rawValue) status=\(status.rawValue) \
            fallback=\(fallback.rawValue) reason=\(permission.reason)
            """
    }
}
