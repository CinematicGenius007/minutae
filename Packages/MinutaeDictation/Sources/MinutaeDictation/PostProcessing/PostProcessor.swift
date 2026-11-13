import Foundation

/// Turns raw speech text into the text that should be inserted.
public protocol PostProcessor: Sendable {
    func process(text: String, bundleID: String?) async -> String
}

extension PostProcessor {
    public func process(text: String) async -> String {
        await process(text: text, bundleID: nil)
    }
}

/// Local cleanup, then the dictionary, then an optional LLM pass.
public struct LocalPostProcessor: PostProcessor {
    private let dictionary: any DictionaryRepository
    private let llm: (any LLMTextCleanup)?
    private let llmEnabled: Bool

    public init(
        dictionary: any DictionaryRepository = InMemoryDictionaryRepository(),
        llm: (any LLMTextCleanup)? = nil,
        llmEnabled: Bool = false
    ) {
        self.dictionary = dictionary
        self.llm = llm
        self.llmEnabled = llmEnabled
    }

    public func process(text: String, bundleID: String?) async -> String {
        let local = locally(text, bundleID: bundleID)
        guard llmEnabled, let llm, !local.isEmpty else { return local }
        do {
            let cleaned = try await llm.cleanup(local)
            let trimmed = cleaned.trimmingCharacters(in: .whitespacesAndNewlines)
            return trimmed.isEmpty ? local : trimmed
        } catch {
            return local
        }
    }

    private func locally(_ text: String, bundleID: String?) -> String {
        let prepared = LocalCleanup.prepare(text)
        let entries = (try? dictionary.entries(forBundleID: bundleID)) ?? []
        let replaced = DictionaryReplacer.apply(prepared, entries: entries, bundleID: bundleID)
        return LocalCleanup.finish(replaced)
    }
}
