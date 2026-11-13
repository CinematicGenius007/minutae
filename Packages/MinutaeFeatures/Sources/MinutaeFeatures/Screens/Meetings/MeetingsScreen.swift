import AVFoundation
import AppKit
import MinutaeCore
import MinutaeDesign
import SwiftUI

public struct MeetingsScreen: View {
    @State private var model: MeetingsViewModel
    @State private var selected: MeetingSelection?

    public init(model: MeetingsViewModel) { _model = State(initialValue: model) }

    private var groups: [(day: Date, meetings: [Meeting])] {
        let calendar = Calendar.current
        let dated = model.filtered.compactMap { m in Formatting.date(m.started_at).map { ($0, m) } }
        let grouped = Dictionary(grouping: dated) { calendar.startOfDay(for: $0.0) }
        return grouped.keys.sorted(by: >).map { day in
            (day, (grouped[day] ?? []).sorted { $0.0 > $1.0 }.map(\.1))
        }
    }

    public var body: some View {
        @Bindable var model = model
        return PageScaffold("Meetings") {
            SearchField(text: $model.search, placeholder: "Search meetings", shortcut: "⌘K")
            if let error = model.error { Text(error).foregroundStyle(Color.minutae.red) }
            if model.filtered.isEmpty {
                VStack(spacing: Spacing.s8) {
                    Text(model.search.isEmpty ? "No meetings yet" : "Nothing matches")
                        .font(.minutae.display(size: 24))
                    Text(model.search.isEmpty
                        ? "Press Record in the sidebar, or join a call and Minutae will offer to record."
                        : "Try a different word.")
                        .font(.minutae.ui(size: 14)).foregroundStyle(Color.minutae.muted)
                        .multilineTextAlignment(.center)
                }
                .frame(maxWidth: .infinity).padding(.top, Spacing.s44)
            } else {
                ForEach(groups, id: \.day) { group in
                    VStack(alignment: .leading, spacing: Spacing.s8) {
                        Text(Formatting.dayHeader(group.day).uppercased())
                            .font(.minutae.mono(size: 12, weight: .medium))
                            .foregroundStyle(Color.minutae.muted)
                            .padding(.top, Spacing.s8)
                        VStack(spacing: Spacing.s4) {
                            ForEach(group.meetings, id: \.id) { meeting in
                                MeetingRow(meeting: meeting, status: model.status(for: meeting)) {
                                    selected = MeetingSelection(meeting: meeting)
                                }
                            }
                        }
                    }
                }
            }
        }
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

private struct MeetingRow: View {
    let meeting: Meeting
    let status: String
    let action: () -> Void
    @State private var hovering = false

    private var tone: StatusPillTone {
        switch status {
        case "Failed": .red
        case "Processing", "Recording": .butter
        default: .mint
        }
    }

    var body: some View {
        Button(action: action) {
            HStack(spacing: Spacing.s16) {
                VStack(alignment: .leading, spacing: Spacing.s4) {
                    Text(meeting.title.isEmpty ? "Untitled meeting" : meeting.title)
                        .font(.minutae.ui(size: 15, weight: .medium))
                        .foregroundStyle(Color.minutae.ink)
                        .lineLimit(1)
                    Text(
                        [Formatting.date(meeting.started_at).map(Formatting.time),
                         Formatting.duration(meeting.duration_seconds)]
                            .compactMap { $0 }.filter { $0 != "—" }.joined(separator: " · ")
                    )
                    .font(.minutae.ui(size: 13)).foregroundStyle(Color.minutae.muted)
                }
                Spacer(minLength: Spacing.s16)
                if status != "Done" { StatusPill(status, tone: tone) }
                Image(systemName: "chevron.right")
                    .font(.minutae.ui(size: 11, weight: .semibold))
                    .foregroundStyle(Color.minutae.muted).accessibilityHidden(true)
            }
            .padding(.horizontal, Spacing.s16).padding(.vertical, Spacing.s12)
            .background(
                hovering ? Color.minutae.surface : Color.minutae.bg,
                in: RoundedRectangle(cornerRadius: Radius.md))
            .overlay(RoundedRectangle(cornerRadius: Radius.md).strokeBorder(Color.minutae.line, lineWidth: 1))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
    }
}

private func duration(_ seconds: Int?) -> String { Formatting.duration(seconds) }

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
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: Spacing.s4) {
                    TextField("Meeting title", text: $title)
                        .textFieldStyle(.plain)
                        .font(.minutae.display(size: 32))
                        .onSubmit { model.rename(title) }
                    Text(
                        "\(Formatting.fullDate(model.meeting.started_at)) · \(Formatting.duration(model.meeting.duration_seconds))"
                    )
                    .font(.minutae.ui(size: 14)).foregroundStyle(Color.minutae.muted)
                }
                Spacer()
                TextAction("Close") { dismiss() }.keyboardShortcut(.cancelAction)
            }
            HStack(spacing: Spacing.s8) {
                SecondaryButton("Copy summary") { copy(model.summary?.markdown_content ?? "") }
                SecondaryButton("Reveal .md") {
                    if let file = model.summaryFile {
                        NSWorkspace.shared.activateFileViewerSelecting([file])
                    }
                }.disabled(model.summaryFile == nil)
                SecondaryButton("Re-run summary") { Task { await model.rerun() } }
                Spacer()
                TextAction("Delete", tone: .destructive) { confirmingDelete = true }
            }
            SegmentedPills(options: tabs, selection: $tab)
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
                                    Button { model.toggle(item) } label: {
                                        HStack(alignment: .top, spacing: Spacing.s12) {
                                            Image(systemName: item.done ? "checkmark.circle.fill" : "circle")
                                                .foregroundStyle(item.done ? Color.minutae.pine : Color.minutae.muted)
                                                .accessibilityHidden(true)
                                            Text(item.text)
                                                .font(.minutae.ui(size: 14))
                                                .strikethrough(item.done)
                                                .foregroundStyle(item.done ? Color.minutae.muted : Color.minutae.ink)
                                                .frame(maxWidth: .infinity, alignment: .leading)
                                        }
                                    }.buttonStyle(.plain)
                                    .accessibilityAddTraits(.isToggle)
                                }
                            }
                        } else {
                            Text("No summary yet. Use Re-run summary once a transcript exists.").font(.minutae.ui(size: 14)).foregroundStyle(Color.minutae.muted)
                        }
                    case "Transcript":
                        SecondaryButton("Copy transcript") { copy(model.transcript?.raw_text ?? "") }
                        ForEach(model.segments, id: \.id) { segment in
                            Button {
                                seek(segment.start_ms)
                            } label: {
                                HStack(alignment: .top, spacing: Spacing.s16) {
                                    Text(Formatting.clock(segment.start_ms / 1000))
                                        .font(.minutae.mono(size: 12))
                                        .foregroundStyle(Color.minutae.muted)
                                    Text(segment.text).frame(
                                        maxWidth: .infinity, alignment: .leading)
                                }
                            }.buttonStyle(.plain)
                        }
                    default:
                        if model.recording != nil {
                            HStack {
                                SecondaryButton("Play") { play() }
                                SecondaryButton("Pause") { player?.pause() }
                            }
                        } else {
                            Text("No recording available.")
                        }
                    }
                }.frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(Spacing.s28).background(Color.minutae.bg).foregroundStyle(Color.minutae.ink)
        .frame(minWidth: 640, idealWidth: 720, minHeight: 520, idealHeight: 640)
        .preferredColorScheme(.light)
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
