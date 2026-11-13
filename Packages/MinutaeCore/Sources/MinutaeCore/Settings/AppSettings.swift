/// Typed app settings. API keys are not fields here; they live in `KeychainStore`.
public struct AppSettings: Hashable, Sendable, Codable {
    public var dictationHotkey: DictationHotkey
    public var keepDictationAudio: Bool
    public var audioRetention: AudioRetention
    public var codexModel: String?
    public var transportFallbackOrder: [TransportID]
    public var apiTransportEnabled: Bool
    public var recordingsPath: String
    public var summariesPath: String
    public var dictationsPath: String
    public var whisperModelName: String
    public var launchAtLogin: Bool
    public var autoDetectMeetings: Bool
    public var promptDelaySeconds: Int

    public static let defaults = AppSettings()

    public init() {
        dictationHotkey = .default
        keepDictationAudio = true
        audioRetention = .never
        codexModel = nil
        transportFallbackOrder = TransportID.defaultFallbackOrder
        apiTransportEnabled = false
        recordingsPath = MinutaeDirectories.recordings.path
        summariesPath = MinutaeDirectories.summaries.path
        dictationsPath = MinutaeDirectories.dictations.path
        whisperModelName = "base"
        launchAtLogin = false
        autoDetectMeetings = true
        promptDelaySeconds = 5
    }

    public func validate() throws {
        try SettingsSanitizer.validateModelName(whisperModelName)
        if let codexModel {
            try SettingsSanitizer.validateModelName(codexModel)
        }
        try SettingsSanitizer.validateStoragePath(recordingsPath)
        try SettingsSanitizer.validateStoragePath(summariesPath)
        try SettingsSanitizer.validateStoragePath(dictationsPath)
        try SettingsSanitizer.validatePromptDelay(promptDelaySeconds)
        try SettingsSanitizer.validateRetention(audioRetention)
    }

    private enum CodingKeys: String, CodingKey {
        case dictationHotkey
        case keepDictationAudio
        case audioRetention
        case codexModel
        case transportFallbackOrder
        case apiTransportEnabled
        case recordingsPath
        case summariesPath
        case dictationsPath
        case whisperModelName
        case launchAtLogin
        case autoDetectMeetings
        case promptDelaySeconds
    }

    public init(from decoder: Decoder) throws {
        self = AppSettings()
        let container = try decoder.container(keyedBy: CodingKeys.self)
        dictationHotkey =
            try container.decodeIfPresent(DictationHotkey.self, forKey: .dictationHotkey)
            ?? dictationHotkey
        keepDictationAudio =
            try container.decodeIfPresent(Bool.self, forKey: .keepDictationAudio)
            ?? keepDictationAudio
        audioRetention =
            try container.decodeIfPresent(AudioRetention.self, forKey: .audioRetention)
            ?? audioRetention
        codexModel = try container.decodeIfPresent(String.self, forKey: .codexModel)
        transportFallbackOrder =
            try container.decodeIfPresent([TransportID].self, forKey: .transportFallbackOrder)
            ?? transportFallbackOrder
        apiTransportEnabled =
            try container.decodeIfPresent(Bool.self, forKey: .apiTransportEnabled)
            ?? apiTransportEnabled
        recordingsPath =
            try container.decodeIfPresent(String.self, forKey: .recordingsPath) ?? recordingsPath
        summariesPath =
            try container.decodeIfPresent(String.self, forKey: .summariesPath) ?? summariesPath
        dictationsPath =
            try container.decodeIfPresent(String.self, forKey: .dictationsPath) ?? dictationsPath
        whisperModelName =
            try container.decodeIfPresent(String.self, forKey: .whisperModelName)
            ?? whisperModelName
        launchAtLogin =
            try container.decodeIfPresent(Bool.self, forKey: .launchAtLogin) ?? launchAtLogin
        autoDetectMeetings =
            try container.decodeIfPresent(Bool.self, forKey: .autoDetectMeetings)
            ?? autoDetectMeetings
        promptDelaySeconds =
            try container.decodeIfPresent(Int.self, forKey: .promptDelaySeconds)
            ?? promptDelaySeconds
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(dictationHotkey, forKey: .dictationHotkey)
        try container.encode(keepDictationAudio, forKey: .keepDictationAudio)
        try container.encode(audioRetention, forKey: .audioRetention)
        try container.encodeIfPresent(codexModel, forKey: .codexModel)
        try container.encode(transportFallbackOrder, forKey: .transportFallbackOrder)
        try container.encode(apiTransportEnabled, forKey: .apiTransportEnabled)
        try container.encode(recordingsPath, forKey: .recordingsPath)
        try container.encode(summariesPath, forKey: .summariesPath)
        try container.encode(dictationsPath, forKey: .dictationsPath)
        try container.encode(whisperModelName, forKey: .whisperModelName)
        try container.encode(launchAtLogin, forKey: .launchAtLogin)
        try container.encode(autoDetectMeetings, forKey: .autoDetectMeetings)
        try container.encode(promptDelaySeconds, forKey: .promptDelaySeconds)
    }
}
