import AVFoundation
import Foundation
import Observation

/// One loaded audio file. `SystemAudioPlayer` wraps `AVAudioPlayer`; tests use a fake.
@MainActor
public protocol AudioPlaying: AnyObject {
    var duration: TimeInterval { get }
    var currentTime: TimeInterval { get set }
    var isPlaying: Bool { get }
    @discardableResult func play() -> Bool
    func pause()
}

public typealias AudioPlayerFactory = @MainActor (URL) throws -> any AudioPlaying

@MainActor
public final class SystemAudioPlayer: AudioPlaying {
    private let player: AVAudioPlayer

    public init(url: URL) throws {
        player = try AVAudioPlayer(contentsOf: url)
        player.prepareToPlay()
    }

    public static func make(_ url: URL) throws -> any AudioPlaying {
        try SystemAudioPlayer(url: url)
    }

    public var duration: TimeInterval { player.duration }

    public var currentTime: TimeInterval {
        get { player.currentTime }
        set { player.currentTime = newValue }
    }

    public var isPlaying: Bool { player.isPlaying }

    @discardableResult
    public func play() -> Bool { player.play() }

    public func pause() { player.pause() }
}

/// Play, pause, and scrub one dictation's WAV. The original file is only read.
@MainActor
@Observable
public final class AudioPlaybackModel {
    public private(set) var isPlaying = false
    public private(set) var currentTime: TimeInterval = 0
    public private(set) var duration: TimeInterval = 0
    public private(set) var isAvailable = false
    public private(set) var unavailableReason: String?

    @ObservationIgnored private var player: (any AudioPlaying)?
    @ObservationIgnored private let makePlayer: AudioPlayerFactory
    @ObservationIgnored private let tick: Duration
    @ObservationIgnored private var ticker: Task<Void, Never>?
    @ObservationIgnored private var loadedPath: String?

    public init(makePlayer: @escaping AudioPlayerFactory, tick: Duration = .milliseconds(100)) {
        self.makePlayer = makePlayer
        self.tick = tick
    }

    /// Loads `path`. `fallbackDuration` fills the label when the file cannot be opened.
    public func load(path: String?, fallbackDuration: TimeInterval?) {
        guard path != loadedPath || player == nil else { return }
        stop()
        loadedPath = path
        player = nil
        currentTime = 0
        duration = fallbackDuration ?? 0
        guard let path, !path.isEmpty else {
            isAvailable = false
            unavailableReason = "No audio was saved for this dictation."
            return
        }
        guard FileManager.default.fileExists(atPath: path) else {
            isAvailable = false
            unavailableReason = "The audio file is missing."
            return
        }
        do {
            let loaded = try makePlayer(URL(fileURLWithPath: path))
            player = loaded
            duration = loaded.duration
            isAvailable = true
            unavailableReason = nil
        } catch {
            isAvailable = false
            unavailableReason = "The audio file could not be opened."
        }
    }

    public var progress: Double {
        guard duration > 0 else { return 0 }
        return min(max(currentTime / duration, 0), 1)
    }

    public func togglePlay() {
        guard let player else { return }
        if player.isPlaying {
            player.pause()
            sync()
            stopTicker()
            return
        }
        if duration > 0, player.currentTime >= duration - 0.01 {
            player.currentTime = 0
        }
        guard player.play() else {
            unavailableReason = "Playback could not start."
            return
        }
        sync()
        startTicker()
    }

    /// `fraction` is clamped to 0...1.
    public func seek(to fraction: Double) {
        guard let player, duration > 0 else { return }
        player.currentTime = min(max(fraction, 0), 1) * duration
        sync()
    }

    /// Moves by `seconds`, for the accessibility adjustable action.
    public func skip(by seconds: TimeInterval) {
        guard duration > 0 else { return }
        seek(to: (currentTime + seconds) / duration)
    }

    public func stop() {
        player?.pause()
        stopTicker()
        isPlaying = false
    }

    /// Reads the player once. The ticker calls this while playing.
    func sync() {
        guard let player else { return }
        isPlaying = player.isPlaying
        currentTime = player.currentTime
    }

    private func startTicker() {
        stopTicker()
        let tick = tick
        ticker = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: tick)
                guard let self else { return }
                self.sync()
                if !self.isPlaying {
                    self.ticker = nil
                    return
                }
            }
        }
    }

    private func stopTicker() {
        ticker?.cancel()
        ticker = nil
    }
}
