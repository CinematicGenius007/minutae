import Foundation
import MinutaeCore

extension DictationController {
    func cleanAndInsert(raw: String, language: String?) async {
        do {
            try writeRow { current in
                current.raw_text = raw
                current.language = language
                current.status = DictationRowStatus.processing
            }
        } catch {
            await fail(message: "store: \(dictationStageDetail(error))")
            return
        }
        if cancelRequested {
            await completeCancel(error: nil)
            return
        }
        let cleaned: String
        do {
            let bundleID = row?.target_app_bundle_id
            cleaned = try await cleaning.process(text: raw, bundleID: bundleID)
        } catch {
            if cancelRequested {
                await completeCancel(error: nil)
                return
            }
            let detail = dictationStageDetail(error)
            await fail(message: "post-process: \(detail)")
            return
        }
        if cancelRequested {
            await completeCancel(error: nil)
            return
        }
        await beginInsert(cleaned)
    }

    func beginInsert(_ cleaned: String) async {
        do {
            try writeRow { current in
                current.final_text = cleaned
                current.status = DictationRowStatus.inserting
            }
        } catch {
            await fail(message: "store: \(dictationStageDetail(error))")
            return
        }
        activity = .inserting
        emit(.inserting, banner: nil)
        if cancelRequested {
            await completeCancel(error: nil)
            return
        }
        await insertCleaned(cleaned)
    }

    func insertCleaned(_ text: String) async {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty {
            await finishDone(method: "none", banner: nil)
            return
        }
        let result = await inserting.insert(text)
        if cancelRequested {
            await completeCancel(error: nil)
            return
        }
        await applyInsertion(result)
    }

    func applyInsertion(_ result: InsertionResult) async {
        switch result {
        case .inserted(let method, _):
            await finishDone(method: dictationInsertMethod(method), banner: nil)
        case .copiedToClipboard(let reason):
            let banner = dictationClipboardBanner(reason)
            await finishDone(method: "clipboardOnly", banner: banner)
        case .refusedSecureField:
            await fail(message: "insert: secure field", method: "none")
        case .failed:
            await fail(message: "insert: failed", method: "none")
        }
    }

    func finishDone(method: String, banner: String?) async {
        do {
            try writeRow { current in
                current.status = DictationRowStatus.done
                current.insert_method = method
                current.error = nil
            }
        } catch {
            await fail(message: "store: \(dictationStageDetail(error))")
            return
        }
        enterIdle(banner: banner)
    }

    func completeCancel(error: String?) async {
        do {
            try writeRow { current in
                current.status = DictationRowStatus.cancelled
                current.error = error
            }
        } catch {
            emit(.error, banner: "store: \(dictationStageDetail(error))")
        }
        enterIdle(banner: "cancelled")
    }

    func fail(message: String, method: String? = nil) async {
        do {
            try writeRow { current in
                current.status = DictationRowStatus.failed
                current.error = message
                if let method {
                    current.insert_method = method
                }
            }
        } catch {
            // The row may stay on the previous status. The HUD still reports this failure.
        }
        emit(.error, banner: message)
        enterIdle(banner: message)
    }

    func enterIdle(banner: String?) {
        activity = .idle
        arming = false
        stopClaimed = false
        cancelRequested = false
        level = 0
        captureTask?.cancel()
        activeRegistration = nil
        emit(.idle, banner: banner)
    }

    func writeRow(_ body: (inout Dictation) -> Void) throws {
        guard var current = row else { return }
        body(&current)
        row = current
        try store.update(current)
    }
}
