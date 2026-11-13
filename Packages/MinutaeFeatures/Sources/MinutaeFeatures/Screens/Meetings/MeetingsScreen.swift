import AVFoundation
import AppKit
import MinutaeCore
import MinutaeDesign
import SwiftUI

public struct MeetingsScreen: View {
    @State private var model: MeetingsViewModel
    @State private var selected: MeetingSelection?

    public init(model: MeetingsViewModel) { _model = State(initialValue: model) }

    public var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: Spacing.s20) {
                Text("Meetings").font(.minutae.display(size: 36))
                TextField("Search meetings", text: $model.search).textFieldStyle(.roundedBorder)
                if let error = model.error { Text(error).foregroundStyle(Color.minutae.red) }
                if model.filtered.isEmpty {
                    ContentUnavailableView(
                        "No meetings", systemImage: "calendar",
                        description: Text("Recorded meetings will appear here."))
                } else {
                    List {
                        ForEach(
                            Array(
                                Dictionary(
                                    grouping: model.filtered,
                                    by: { String($0.started_at.prefix(10)) }
                                ).keys.sorted(by: >)), id: \.self
                        ) { day in
                            Section(day) {
                                ForEach(
                                    model.filtered.filter { $0.started_at.hasPrefix(day) }, id: \.id
                                ) { meeting in
                                    Button {
                                        selected = MeetingSelection(meeting: meeting)
                                    } label: {
                                        MeetingRow(
                                            meeting: meeting, status: model.status(for: meeting))
                                    }.buttonStyle(.plain)
                                }
                            }
                        }
                    }
                }
            }
            .padding(Spacing.s28).background(Color.minutae.bg).foregroundStyle(Color.minutae.ink)
            .sheet(item: $selected) { selection in
                MeetingDetailScreen(
                    model: MeetingDetailViewModel(
                        meeting: selection.meeting, database: model.database, runner: model.runner,
                        summariesDirectory: model.summariesDirectory))
            }
            .task {
                while !Task.isCancelled {
                    model.refresh()
                    try? await Task.sleep(for: .seconds(2))
                }
            }
        }
    }
}

private struct MeetingRow: View {
    let meeting: Meeting
    let status: String
    var body: some View {
        HStack {
            VStack(alignment: .leading) {
                Text(meeting.title).font(.minutae.ui(size: 16, weight: .medium))
                Text("\(meeting.started_at) · \(duration(meeting.duration_seconds))")
                    .font(.minutae.mono(size: 12)).foregroundStyle(Color.minutae.muted)
            }
            Spacer()
            Text(status).font(.minutae.ui(size: 12, weight: .regular))
                .padding(Spacing.s8).background(Color.minutae.mint, in: Capsule())
        }
    }
}

private func duration(_ seconds: Int?) -> String {
    guard let seconds else { return "—" }
    return String(format: "%d:%02d", seconds / 60, seconds % 60)
}

private struct MeetingSelection: Identifiable {
    let meeting: Meeting
    var id: String { meeting.id }
}

public struct MeetingDetailScreen: View {
    @Environment(\.dismiss) private var dismiss
    @State private var model: MeetingDetailViewModel
    @State private var title: String
    @State private var tab = "Summary"
    @State private var confirmingDelete = false
    @State private var player: AVAudioPlayer?
    private let tabs = ["Summary", "Transcript", "Audio"]

    public init(model: MeetingDetailViewModel) {
        _model = State(initialValue: model)
        _title = State(initialValue: model.meeting.title)
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s20) {
            TextField("Meeting title", text: $title).font(.minutae.display(size: 32)).onSubmit {
                model.rename(title)
            }
            Text("\(model.meeting.started_at) · \(duration(model.meeting.duration_seconds))")
                .foregroundStyle(Color.minutae.muted)
            HStack {
                Button("Copy summary") { copy(model.summary?.markdown_content ?? "") }
                Button("Reveal .md") {
                    if let file = model.summaryFile {
                        NSWorkspace.shared.activateFileViewerSelecting([file])
                    }
                }.disabled(model.summaryFile == nil)
                Button("Re-run summary") { Task { await model.rerun() } }
                Button("Delete", role: .destructive) { confirmingDelete = true }
            }
            Picker("Meeting content", selection: $tab) { ForEach(tabs, id: \.self) { Text($0) } }
                .pickerStyle(.segmented)
            if let error = model.error { Text(error).foregroundStyle(Color.minutae.red) }
            ScrollView {
                VStack(alignment: .leading, spacing: Spacing.s16) {
                    switch tab {
                    case "Summary":
                        if let summary = model.summary {
                            let content =
                                summary.markdown_content.components(separatedBy: "## Action Items")
                                .first ?? summary.markdown_content
                            Text(
                                (try? AttributedString(markdown: content))
                                    ?? AttributedString(content)
                            ).textSelection(.enabled)
                            if !model.items.isEmpty {
                                Text("Action Items").font(.minutae.display(size: 24))
                                ForEach(model.items, id: \.id) { item in
                                    Toggle(
                                        item.text,
                                        isOn: Binding(
                                            get: { item.done }, set: { _ in model.toggle(item) })
                                    ).toggleStyle(.checkbox)
                                }
                            }
                        } else {
                            Text("No summary yet.")
                        }
                    case "Transcript":
                        Button("Copy transcript") { copy(model.transcript?.raw_text ?? "") }
                        ForEach(model.segments, id: \.id) { segment in
                            Button {
                                seek(segment.start_ms)
                            } label: {
                                HStack(alignment: .top, spacing: Spacing.s16) {
                                    Text(duration(segment.start_ms / 1000)).font(
                                        .minutae.mono(size: 12))
                                    Text(segment.text).frame(
                                        maxWidth: .infinity, alignment: .leading)
                                }
                            }.buttonStyle(.plain)
                        }
                    default:
                        if model.recording != nil {
                            HStack {
                                Button("Play") { play() }
                                Button("Pause") { player?.pause() }
                            }
                        } else {
                            Text("No recording available.")
                        }
                    }
                }.frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(Spacing.s28).background(Color.minutae.bg).foregroundStyle(Color.minutae.ink)
        .confirmationDialog("Delete this meeting and its files?", isPresented: $confirmingDelete) {
            Button("Delete meeting", role: .destructive) {
                model.delete()
                if model.deleted { dismiss() }
            }
        }
    }

    private func copy(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }
    private func prepare() {
        if player == nil, let path = model.recording?.file_path {
            player = try? AVAudioPlayer(contentsOf: URL(fileURLWithPath: path))
        }
    }
    private func play() {
        prepare()
        player?.play()
    }
    private func seek(_ milliseconds: Int) {
        prepare()
        player?.currentTime = Double(milliseconds) / 1000
        player?.play()
    }
}
