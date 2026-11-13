import CryptoKit
import Foundation

public enum ModelError: Error { case invalidName, unsupportedModel, badResponse, invalidSize, invalidChecksum }
public protocol ModelDownloading: Sendable {
    func download(from url: URL, progress: @escaping @Sendable (Double) -> Void) async throws -> URL
}
public struct URLSessionModelDownloader: ModelDownloading {
    public init() {}
    public func download(from url: URL, progress: @escaping @Sendable (Double) -> Void) async throws -> URL {
        let delegate = DownloadProgress(progress: progress)
        let session = URLSession(configuration: .ephemeral, delegate: delegate, delegateQueue: nil)
        defer { session.finishTasksAndInvalidate() }
        let (temporary, response) = try await session.download(from: url)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200, let url = http.url, isOfficialDownloadURL(url) else { throw ModelError.badResponse }
        let destination = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.moveItem(at: temporary, to: destination)
        return destination
    }
}
private func isOfficialDownloadURL(_ url: URL) -> Bool {
    guard url.scheme == "https", let host = url.host?.lowercased() else { return false }
    return host == "huggingface.co" || host.hasSuffix(".huggingface.co") || host.hasSuffix(".hf.co")
}
private final class DownloadProgress: NSObject, URLSessionDownloadDelegate, Sendable {
    let progress: @Sendable (Double) -> Void
    init(progress: @escaping @Sendable (Double) -> Void) { self.progress = progress }
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest, completionHandler: @escaping @Sendable (URLRequest?) -> Void) {
        completionHandler(request.url.map(isOfficialDownloadURL) == true ? request : nil)
    }
    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask, didFinishDownloadingTo location: URL) {}
    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask, didWriteData bytesWritten: Int64, totalBytesWritten: Int64, totalBytesExpectedToWrite: Int64) {
        if totalBytesExpectedToWrite > 0 { progress(Double(totalBytesWritten) / Double(totalBytesExpectedToWrite)) }
    }
}
public struct ModelManager: Sendable {
    private struct Model: Sendable { let size: Int64; let sha: String }
    private static let models: [String: Model] = [
        "tiny": Model(size: 77691713, sha: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21"),
        "base": Model(size: 147951465, sha: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe"),
        "small": Model(size: 487601967, sha: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"),
        "medium": Model(size: 1533763059, sha: "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208")
    ]
    public let directory: URL
    private let downloader: any ModelDownloading
    public init(directory: URL = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Minutae/models", isDirectory: true), downloader: any ModelDownloading = URLSessionModelDownloader()) {
        self.directory = directory; self.downloader = downloader
    }
    public func modelURL(name: String) throws -> URL {
        guard !name.isEmpty, name.utf8.allSatisfy({ (97...122).contains($0) || (48...57).contains($0) || $0 == 45 }), Self.models[name] != nil else { throw ModelError.invalidName }
        return directory.appendingPathComponent("ggml-\(name).bin", isDirectory: false)
    }
    public func isInstalled(name: String) throws -> Bool { FileManager.default.fileExists(atPath: try modelURL(name: name).path) }
    public func download(name: String, progress: @escaping @Sendable (Double) -> Void = { _ in }) async throws -> URL {
        let destination = try modelURL(name: name)
        guard let model = Self.models[name] else { throw ModelError.unsupportedModel }
        let source = URL(string: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-\(name).bin")!
        let temporary = try await downloader.download(from: source, progress: progress)
        defer { try? FileManager.default.removeItem(at: temporary) }
        let attributes = try FileManager.default.attributesOfItem(atPath: temporary.path)
        guard (attributes[.size] as? NSNumber)?.int64Value == model.size else { throw ModelError.invalidSize }
        let file = try FileHandle(forReadingFrom: temporary)
        defer { try? file.close() }
        var hasher = SHA256()
        while let data = try file.read(upToCount: 1024 * 1024), !data.isEmpty { hasher.update(data: data) }
        let hash = hasher.finalize().map { String(format: "%02x", $0) }.joined()
        guard hash == model.sha else { throw ModelError.invalidChecksum }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let staging = directory.appendingPathComponent(".\(UUID().uuidString).download")
        defer { try? FileManager.default.removeItem(at: staging) }
        try FileManager.default.copyItem(at: temporary, to: staging)
        guard rename(staging.path, destination.path) == 0 else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
        return destination
    }
}
