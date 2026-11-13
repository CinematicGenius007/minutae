import AppKit
import MinutaeCore
import MinutaeDesign
import MinutaeDictation
import SwiftUI

@MainActor
@Observable
final class DebugHarnessModel {
    var bootError: String?
    var environment: AppEnvironment?
    var notes: [String] = []
    var permissions: [PermissionSnapshot] = []
    var modelLine = "Checking the Whisper model…"
    var downloadProgress: Double?
    var transportLines: [TransportLine] = []
    var testResults: [TransportID: String] = [:]
    var meetings: [Meeting] = []
    var dictations: [Dictation] = []
    var jobs: [Job] = []
    var runnerErrors: [String] = []
    var listError: String?
    var actionMessage: String?
    var hudText = "Dictation is idle."
    var codexModelDraft = ""
    var recordingMeetingID: String?
    var recordingStarted: Date?
    var recordingBusy = false
    var modelBusy = false
    private var didBoot = false
    private var tick = 0
    private let hud = DictationHUDPanel()

    var elapsedText: String {
        guard let recordingStarted else { return "0:00" }
        _ = tick
        return ElapsedTime.text(since: recordingStarted)
    }

    func bootIfNeeded() async {
        guard !didBoot else { return }
        didBoot = true
        if AppLaunch.isUnderTest { return }
        do {
            let environment = try await AppEnvironment.makeFromProcess()
            self.environment = environment
            notes = environment.startupNotes
            codexModelDraft = environment.settings.codexModel ?? ""
            startWatchers(environment)
        } catch {
            bootError = AppMessages.describe(error)
        }
    }

    func request(_ permission: Permission) async {
        guard let environment else { return }
        let status = await environment.permissions.request(permission)
        if permission == .microphone, status == .granted, let message = environment.prepareMicrophone() {
            actionMessage = message
        }
        if status != .granted {
            actionMessage = AppMessages.permissionBlocked(permission, status: status)
        }
        await refresh()
    }

    func refreshTransports() async {
        guard let environment else { return }
        transportLines = await environment.transportLines()
    }

    func testTransport(_ id: TransportID) async {
        guard let environment else { return }
        testResults[id] = "Testing \(AppMessages.transportName(id))…"
        testResults[id] = await environment.testTransport(id)
    }

    func saveCodexModel() async {
        guard let environment else { return }
        do {
            try await environment.setCodexModel(codexModelDraft)
            actionMessage = codexModelDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                ? "Cleared the Codex model. A test will ask you to set one."
                : "Saved the Codex model."
            await refreshTransports()
        } catch {
            actionMessage = AppMessages.describe(error)
        }
    }

    func downloadOrLoadModel() async {
        guard let environment, !modelBusy else { return }
        modelBusy = true
        downloadProgress = 0
        defer {
            modelBusy = false
            downloadProgress = nil
        }
        do {
            try await environment.ensureWhisperModel { [weak self] progress in
                Task { @MainActor in self?.downloadProgress = progress }
            }
            actionMessage = "The Whisper model is ready."
        } catch {
            actionMessage = AppMessages.describe(error)
        }
        modelLine = await environment.modelStatusLine()
    }

    func toggleRecording() async {
        guard let environment, !recordingBusy else { return }
        recordingBusy = true
        defer { recordingBusy = false }
        if recordingMeetingID != nil {
            do {
                _ = try await environment.meetings.stop()
                recordingMeetingID = nil
                recordingStarted = nil
                actionMessage = "Recording stopped. Transcription and summary jobs are queued."
            } catch {
                actionMessage = AppMessages.describe(error)
            }
            await refresh()
            return
        }
        if !environment.usesFakes {
            let status = environment.permissions.status(of: .microphone)
            if status != .granted {
                actionMessage = AppMessages.permissionBlocked(.microphone, status: status)
                if status == .notDetermined {
                    await request(.microphone)
                }
                return
            }
            if let message = environment.prepareMicrophone() {
                actionMessage = message
                return
            }
        }
        let modelLine = await environment.modelStatusLine()
        self.modelLine = modelLine
        let modelWarning = (modelLine.contains("not installed") || modelLine.contains("not loaded"))
            ? modelLine : nil
        do {
            let id = try await environment.meetings.start(title: "Meeting")
            recordingMeetingID = id
            recordingStarted = Date()
            actionMessage = modelWarning.map { "Recording. \($0)" } ?? "Recording."
        } catch {
            actionMessage = AppMessages.describe(error)
        }
        await refresh()
    }

    func reveal(_ path: String?) {
        guard let path, !path.isEmpty else {
            actionMessage = "There is no file to show yet."
            return
        }
        let url = URL(fileURLWithPath: path)
        guard FileManager.default.fileExists(atPath: path) else {
            actionMessage = "That file is not on disk yet."
            return
        }
        NSWorkspace.shared.activateFileViewerSelecting([url])
    }

    func cancelDictation() async {
        await environment?.dictation.cancel()
    }

    private func startWatchers(_ environment: AppEnvironment) {
        let dictation = environment.dictation
        let panel = hud
        Task {
            let events = await dictation.events()
            for await event in events {
                switch event {
                case .state(let state):
                    panel.apply(state)
                    hudText = DictationHUDCopy.line(state)
                case .ignoredWhileBusy:
                    actionMessage = "That press was ignored because a dictation is already running."
                }
            }
        }
        let runner = environment.jobRunner
        Task {
            let errors = await runner.databaseErrors()
            for await message in errors {
                runnerErrors.append(message)
            }
        }
        Task {
            while !Task.isCancelled {
                await refresh()
                try? await Task.sleep(for: .milliseconds(700))
            }
        }
        Task {
            await refreshTransports()
        }
    }

    private func refresh() async {
        guard let environment else { return }
        do {
            meetings = try Repository<Meeting>(environment.database).newestFirst()
            dictations = try Repository<Dictation>(environment.database).all().sorted { left, right in
                left.started_at > right.started_at
            }
            jobs = try Repository<Job>(environment.database).all().sorted { left, right in
                left.updated_at > right.updated_at
            }
            listError = nil
        } catch {
            listError = AppMessages.describe(error)
        }
        permissions = Permission.allCases.map { permission in
            PermissionSnapshot(
                permission: permission,
                status: environment.permissions.status(of: permission)
            )
        }
        modelLine = await environment.modelStatusLine()
        tick += 1
    }
}

struct PermissionSnapshot: Identifiable {
    var permission: Permission
    var status: PermissionStatus
    var id: String { permission.rawValue }
}

enum ElapsedTime {
    static func text(since start: Date, now: Date = Date()) -> String {
        let seconds = max(0, Int(now.timeIntervalSince(start)))
        return String(format: "%d:%02d", seconds / 60, seconds % 60)
    }
}

enum DictationHUDCopy {
    static func line(_ state: DictationHUDState) -> String {
        let phase: String
        switch state.phase {
        case .idle:
            phase = "Idle"
        case .listening:
            phase = "Listening"
        case .processing:
            phase = "Processing"
        case .inserting:
            phase = "Inserting"
        case .error:
            phase = "Error"
        }
        if let banner = state.banner, !banner.isEmpty {
            return "\(phase). \(banner)"
        }
        if let name = state.targetAppName, !name.isEmpty {
            return "\(phase) · \(name)"
        }
        return phase
    }
}

@MainActor
final class DictationHUDPanel {
    private let panel: NSPanel
    private let label: NSTextField

    init() {
        panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 320, height: 64),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.level = .statusBar
        panel.isFloatingPanel = true
        panel.hidesOnDeactivate = false
        panel.isOpaque = false
        panel.hasShadow = true
        panel.ignoresMouseEvents = true
        panel.becomesKeyOnlyIfNeeded = true
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        panel.backgroundColor = .windowBackgroundColor
        label = NSTextField(labelWithString: "Idle")
        label.frame = NSRect(x: 16, y: 18, width: 288, height: 28)
        label.font = .systemFont(ofSize: 13, weight: .medium)
        label.textColor = .labelColor
        let content = NSView(frame: NSRect(x: 0, y: 0, width: 320, height: 64))
        content.addSubview(label)
        panel.contentView = content
    }

    func apply(_ state: DictationHUDState) {
        label.stringValue = DictationHUDCopy.line(state)
        if state.phase == .idle, state.banner == nil {
            panel.orderOut(nil)
            return
        }
        if let screen = NSScreen.main ?? NSScreen.screens.first {
            let visible = screen.visibleFrame
            let origin = NSPoint(x: visible.midX - 160, y: visible.minY + 48)
            panel.setFrameOrigin(origin)
        }
        panel.orderFrontRegardless()
    }
}

struct DebugHarnessView: View {
    @Bindable var model: DebugHarnessModel
    @State private var showsDesignGallery = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                header
                if let bootError = model.bootError {
                    Text(bootError).foregroundStyle(.red)
                }
                if model.environment != nil {
                    notesSection
                    permissionsSection
                    modelSection
                    transportSection
                    recordSection
                    meetingsSection
                    dictationsSection
                    jobsSection
                } else if model.bootError == nil {
                    Text("Starting…")
                }
            }
            .padding(20)
            .frame(maxWidth: 760, alignment: .leading)
        }
        .frame(minWidth: 680, minHeight: 520)
        .background(Color.white)
        .preferredColorScheme(.light)
        .task { await model.bootIfNeeded() }
        .sheet(isPresented: $showsDesignGallery) {
            DesignGalleryView()
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Minutae").font(.title2)
            Text(model.environment?.usesFakes == true ? "Debug harness · fakes" : "Debug harness")
                .font(.subheadline)
                .foregroundStyle(.secondary)
            Button("Design gallery") { showsDesignGallery = true }
            if let root = model.environment?.root {
                Text(root.path).font(.caption).textSelection(.enabled).foregroundStyle(.secondary)
            }
        }
    }

    private var notesSection: some View {
        VStack(alignment: .leading, spacing: 4) {
            if let action = model.actionMessage {
                Text(action).textSelection(.enabled)
            }
            ForEach(Array(model.notes.enumerated()), id: \.offset) { _, note in
                Text(note).font(.callout).textSelection(.enabled)
            }
        }
    }

    private var permissionsSection: some View {
        section("Permissions") {
            ForEach(model.permissions) { row in
                HStack {
                    Text("\(AppMessages.permissionName(row.permission)): \(AppMessages.statusName(row.status))")
                    Spacer()
                    Button("Request") { Task { await model.request(row.permission) } }
                    Button("Settings") { _ = PermissionSettingsLink.open(row.permission) }
                }
            }
            Text("Dictation: \(model.hudText)").font(.callout)
            Button("Cancel dictation") { Task { await model.cancelDictation() } }
        }
    }

    private var modelSection: some View {
        section("Whisper") {
            Text(model.modelLine).textSelection(.enabled)
            if let progress = model.downloadProgress {
                ProgressView(value: progress)
            }
            Button(model.modelBusy ? "Working…" : "Download or load model") {
                Task { await model.downloadOrLoadModel() }
            }
            .disabled(model.modelBusy)
        }
    }

    private var transportSection: some View {
        section("AI transports") {
            HStack {
                TextField("Codex model", text: $model.codexModelDraft)
                    .textFieldStyle(.roundedBorder)
                Button("Save") { Task { await model.saveCodexModel() } }
            }
            Button("Refresh status") { Task { await model.refreshTransports() } }
            ForEach(model.transportLines) { line in
                VStack(alignment: .leading, spacing: 4) {
                    Text("\(AppMessages.transportName(line.id)): \(line.text)")
                        .textSelection(.enabled)
                    if let result = model.testResults[line.id] {
                        Text(result).font(.callout).textSelection(.enabled)
                    }
                    Button("Test") { Task { await model.testTransport(line.id) } }
                }
            }
        }
    }

    private var recordSection: some View {
        section("Meeting") {
            HStack {
                Button(model.recordingMeetingID == nil ? "Record meeting" : "Stop") {
                    Task { await model.toggleRecording() }
                }
                .disabled(model.recordingBusy)
                if model.recordingMeetingID != nil {
                    Text(model.elapsedText).monospacedDigit()
                }
            }
        }
    }

    private var meetingsSection: some View {
        section("Meetings") {
            if model.meetings.isEmpty {
                Text("No meetings yet.").foregroundStyle(.secondary)
            }
            ForEach(model.meetings, id: \.id) { meeting in
                MeetingRow(meeting: meeting, model: model)
            }
        }
    }

    private var dictationsSection: some View {
        section("Dictations") {
            if model.dictations.isEmpty {
                Text("No dictations yet.").foregroundStyle(.secondary)
            }
            ForEach(model.dictations, id: \.id) { row in
                VStack(alignment: .leading, spacing: 2) {
                    Text("\(row.started_at) · \(row.status) · \(row.insert_method ?? "no insert method")")
                    Text(row.final_text.isEmpty ? row.raw_text : row.final_text)
                        .textSelection(.enabled)
                    if let audio = row.audio_path {
                        Text(audio).font(.caption).textSelection(.enabled)
                    }
                    if let error = row.error, !error.isEmpty {
                        Text(error).foregroundStyle(.red).textSelection(.enabled)
                    }
                }
            }
        }
    }

    private var jobsSection: some View {
        section("Job errors") {
            if let listError = model.listError {
                Text(listError).foregroundStyle(.red)
            }
            let failed = model.jobs.filter { job in
                job.state == "failed" || !(job.last_error ?? "").isEmpty
            }
            if failed.isEmpty, model.runnerErrors.isEmpty {
                Text("No job errors.").foregroundStyle(.secondary)
            }
            ForEach(failed.prefix(20), id: \.id) { job in
                Text("\(job.kind) \(job.state): \(job.last_error ?? "")")
                    .textSelection(.enabled)
            }
            ForEach(model.runnerErrors.suffix(10), id: \.self) { message in
                Text(message).textSelection(.enabled)
            }
        }
    }

    private func section<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title).font(.headline)
            content()
        }
    }
}

private struct MeetingRow: View {
    var meeting: Meeting
    var model: DebugHarnessModel

    var body: some View {
        let path = model.environment?.summaryFile(meetingID: meeting.id)?.path
        VStack(alignment: .leading, spacing: 2) {
            Text("\(meeting.title) · \(meeting.status)")
            Text(path ?? "No summary file yet.")
                .font(.caption)
                .textSelection(.enabled)
            Button("Open in Finder") { model.reveal(path) }
                .disabled(path == nil)
        }
    }
}
