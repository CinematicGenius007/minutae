import Foundation
import GRDB

public struct Meeting: Codable, FetchableRecord, PersistableRecord, TableRecord, Sendable {
    public static let databaseTableName = "meetings"
    public var id: String
    public var title: String
    public var started_at: String
    public var ended_at: String?
    public var duration_seconds: Int?
    public var status: String
    public init(id: String, title: String, started_at: String, ended_at: String?, duration_seconds: Int?, status: String) {
        self.id = id
        self.title = title
        self.started_at = started_at
        self.ended_at = ended_at
        self.duration_seconds = duration_seconds
        self.status = status
    }
}

public struct Recording: Codable, FetchableRecord, PersistableRecord, TableRecord, Sendable {
    public static let databaseTableName = "recordings"
    public var id: String
    public var meeting_id: String
    public var file_path: String
    public var file_size_bytes: Int?
    public var created_at: String
    public init(id: String, meeting_id: String, file_path: String, file_size_bytes: Int?, created_at: String) {
        self.id = id
        self.meeting_id = meeting_id
        self.file_path = file_path
        self.file_size_bytes = file_size_bytes
        self.created_at = created_at
    }
}

public struct Transcript: Codable, FetchableRecord, PersistableRecord, TableRecord, Sendable {
    public static let databaseTableName = "transcripts"
    public var id: String
    public var meeting_id: String
    public var raw_text: String
    public var provider: String
    public var language: String?
    public var duration_ms: Int?
    public var created_at: String
    public init(id: String, meeting_id: String, raw_text: String, provider: String, language: String?, duration_ms: Int?, created_at: String) {
        self.id = id
        self.meeting_id = meeting_id
        self.raw_text = raw_text
        self.provider = provider
        self.language = language
        self.duration_ms = duration_ms
        self.created_at = created_at
    }
}

public struct Segment: Codable, FetchableRecord, PersistableRecord, TableRecord, Sendable {
    public static let databaseTableName = "segments"
    public var id: String
    public var transcript_id: String
    public var start_ms: Int
    public var end_ms: Int
    public var text: String
    public var speaker_label: String?
    public init(id: String, transcript_id: String, start_ms: Int, end_ms: Int, text: String, speaker_label: String?) {
        self.id = id
        self.transcript_id = transcript_id
        self.start_ms = start_ms
        self.end_ms = end_ms
        self.text = text
        self.speaker_label = speaker_label
    }
}

public struct Summary: Codable, FetchableRecord, PersistableRecord, TableRecord, Sendable {
    public static let databaseTableName = "summaries"
    public var id: String
    public var meeting_id: String
    public var markdown_content: String
    public var model: String
    public var provider: String
    public var created_at: String
    public init(id: String, meeting_id: String, markdown_content: String, model: String, provider: String, created_at: String) {
        self.id = id
        self.meeting_id = meeting_id
        self.markdown_content = markdown_content
        self.model = model
        self.provider = provider
        self.created_at = created_at
    }
}

public struct ActionItem: Codable, FetchableRecord, PersistableRecord, TableRecord, Sendable {
    public static let databaseTableName = "action_items"
    public var id: String
    public var meeting_id: String
    public var text: String
    public var done: Bool
    public var created_at: String
    public init(id: String, meeting_id: String, text: String, done: Bool, created_at: String) {
        self.id = id
        self.meeting_id = meeting_id
        self.text = text
        self.done = done
        self.created_at = created_at
    }
}

public struct Embedding: Codable, FetchableRecord, PersistableRecord, TableRecord, Sendable {
    public static let databaseTableName = "embeddings"
    public var id: String
    public var meeting_id: String
    public var chunk_text: String
    public var chunk_index: Int
    public var vector: Data
    public init(id: String, meeting_id: String, chunk_text: String, chunk_index: Int, vector: Data) {
        self.id = id
        self.meeting_id = meeting_id
        self.chunk_text = chunk_text
        self.chunk_index = chunk_index
        self.vector = vector
    }
}

public struct APIUsageLog: Codable, FetchableRecord, PersistableRecord, TableRecord, Sendable {
    public static let databaseTableName = "api_usage_log"
    public var id: String
    public var meeting_id: String?
    public var provider: String
    public var model: String
    public var endpoint: String
    public var input_tokens: Int
    public var output_tokens: Int
    public var cost_usd: Double
    public var created_at: String
    public var transport: String?
    public var duration_ms: Int?
    public var error_class: String?
    public init(id: String, meeting_id: String?, provider: String, model: String, endpoint: String, input_tokens: Int, output_tokens: Int, cost_usd: Double, created_at: String, transport: String?, duration_ms: Int?, error_class: String?) {
        self.id = id
        self.meeting_id = meeting_id
        self.provider = provider
        self.model = model
        self.endpoint = endpoint
        self.input_tokens = input_tokens
        self.output_tokens = output_tokens
        self.cost_usd = cost_usd
        self.created_at = created_at
        self.transport = transport
        self.duration_ms = duration_ms
        self.error_class = error_class
    }
}

public struct Setting: Codable, FetchableRecord, PersistableRecord, TableRecord, Sendable {
    public static let databaseTableName = "settings"
    public var key: String
    public var value: String
    public var updated_at: String
    public init(key: String, value: String, updated_at: String) {
        self.key = key
        self.value = value
        self.updated_at = updated_at
    }
}

public struct Dictation: Codable, FetchableRecord, PersistableRecord, TableRecord, Sendable {
    public static let databaseTableName = "dictations"
    public var id: String
    public var started_at: String
    public var duration_ms: Int?
    public var target_app_bundle_id: String?
    public var target_app_name: String?
    public var raw_text: String
    public var final_text: String
    public var language: String?
    public var insert_method: String?
    public var status: String
    public var audio_path: String?
    public var error: String?
    public var created_at: String
    public init(id: String, started_at: String, duration_ms: Int?, target_app_bundle_id: String?, target_app_name: String?, raw_text: String, final_text: String, language: String?, insert_method: String?, status: String, audio_path: String?, error: String?, created_at: String) {
        self.id = id
        self.started_at = started_at
        self.duration_ms = duration_ms
        self.target_app_bundle_id = target_app_bundle_id
        self.target_app_name = target_app_name
        self.raw_text = raw_text
        self.final_text = final_text
        self.language = language
        self.insert_method = insert_method
        self.status = status
        self.audio_path = audio_path
        self.error = error
        self.created_at = created_at
    }
}

public struct Job: Codable, FetchableRecord, PersistableRecord, TableRecord, Sendable {
    public static let databaseTableName = "jobs"
    public var id: String
    public var kind: String
    public var subject_type: String
    public var subject_id: String
    public var payload: String?
    public var state: String
    public var attempts: Int
    public var max_attempts: Int
    public var last_error: String?
    public var run_after: String?
    public var created_at: String
    public var updated_at: String
    public init(id: String, kind: String, subject_type: String, subject_id: String, payload: String?, state: String, attempts: Int, max_attempts: Int, last_error: String?, run_after: String?, created_at: String, updated_at: String) {
        self.id = id
        self.kind = kind
        self.subject_type = subject_type
        self.subject_id = subject_id
        self.payload = payload
        self.state = state
        self.attempts = attempts
        self.max_attempts = max_attempts
        self.last_error = last_error
        self.run_after = run_after
        self.created_at = created_at
        self.updated_at = updated_at
    }
}

public struct Tag: Codable, FetchableRecord, PersistableRecord, TableRecord, Sendable {
    public static let databaseTableName = "tags"
    public var id: String
    public var name: String
    public var color: String?
    public var created_at: String
    public init(id: String, name: String, color: String?, created_at: String) {
        self.id = id
        self.name = name
        self.color = color
        self.created_at = created_at
    }
}

public struct ItemTag: Codable, FetchableRecord, PersistableRecord, TableRecord, Sendable {
    public static let databaseTableName = "item_tags"
    public var tag_id: String
    public var subject_type: String
    public var subject_id: String
    public init(tag_id: String, subject_type: String, subject_id: String) {
        self.tag_id = tag_id
        self.subject_type = subject_type
        self.subject_id = subject_id
    }
}

public struct DictionaryEntry: Codable, FetchableRecord, PersistableRecord, TableRecord, Sendable {
    public static let databaseTableName = "dictionary"
    public var id: String
    public var term: String
    public var replacement: String
    public var kind: String
    public var scope_bundle_id: String?
    public var enabled: Bool
    public var created_at: String
    public init(id: String, term: String, replacement: String, kind: String, scope_bundle_id: String?, enabled: Bool, created_at: String) {
        self.id = id
        self.term = term
        self.replacement = replacement
        self.kind = kind
        self.scope_bundle_id = scope_bundle_id
        self.enabled = enabled
        self.created_at = created_at
    }
}

public struct TranscriptEdit: Codable, FetchableRecord, PersistableRecord, TableRecord, Sendable {
    public static let databaseTableName = "transcript_edits"
    public var id: String
    public var segment_id: String
    public var kind: String
    public var original_text: String
    public var edited_text: String
    public var edited_at: String
    public init(id: String, segment_id: String, kind: String, original_text: String, edited_text: String, edited_at: String) {
        self.id = id
        self.segment_id = segment_id
        self.kind = kind
        self.original_text = original_text
        self.edited_text = edited_text
        self.edited_at = edited_at
    }
}
