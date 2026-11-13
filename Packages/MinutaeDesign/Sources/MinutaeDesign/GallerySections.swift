import SwiftUI

struct GalleryColors: View {
    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s12) {
            SectionLabel("Colour")
            LazyVGrid(
                columns: [GridItem(.adaptive(minimum: 148), spacing: Spacing.s12)],
                alignment: .leading,
                spacing: Spacing.s12
            ) {
                ForEach(MinutaePalette.all) { token in
                    ColorSwatch(token: token)
                }
            }
        }
    }
}

private struct ColorSwatch: View {
    var token: MinutaeColorToken

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s8) {
            RoundedRectangle(cornerRadius: Radius.sm)
                .fill(token.color)
                .frame(height: Spacing.s44)
                .overlay {
                    RoundedRectangle(cornerRadius: Radius.sm)
                        .strokeBorder(Color.minutae.line, lineWidth: 1)
                }
            Text(caption)
                .font(.minutae.ui(size: 13, weight: .medium))
                .foregroundStyle(Color.minutae.ink)
            Text(token.hexString)
                .font(.minutae.mono(size: 12))
                .foregroundStyle(Color.minutae.muted)
        }
    }

    private var caption: String {
        if token.name == "white" { return "white · inverse text" }
        if let paperName = token.paperName { return "\(token.name) · \(paperName)" }
        return token.name
    }
}

struct GalleryType: View {
    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s12) {
            SectionLabel("Type")
            Text("Fraunces for titles")
                .font(.minutae.display(size: 28, italic: true))
                .foregroundStyle(Color.minutae.ink)
            Text("Geist is the interface, in weights from regular to semibold.")
                .font(.minutae.ui(size: 16))
                .foregroundStyle(Color.minutae.ink)
            Text("Geist Medium")
                .font(.minutae.ui(size: 16, weight: .medium))
                .foregroundStyle(Color.minutae.ink)
            Text("Geist Mono  0123456789")
                .font(.minutae.mono(size: 13))
                .foregroundStyle(Color.minutae.ink)
        }
    }
}

struct GalleryMetrics: View {
    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s12) {
            SectionLabel("Spacing and radius")
            VStack(alignment: .leading, spacing: Spacing.s8) {
                ForEach(Array(Spacing.all.enumerated()), id: \.offset) { _, value in
                    SpacingSample(value: value)
                }
            }
            HStack(spacing: Spacing.s16) {
                RadiusSample(title: "sm 8", radius: Radius.sm)
                RadiusSample(title: "md 12", radius: Radius.md)
                VStack(alignment: .leading, spacing: Spacing.s8) {
                    Capsule()
                        .fill(Color.minutae.pine)
                        .frame(width: 72, height: 28)
                    Text("pill")
                        .font(.minutae.mono(size: 12))
                        .foregroundStyle(Color.minutae.muted)
                }
            }
        }
    }
}

private struct SpacingSample: View {
    var value: CGFloat

    var body: some View {
        HStack(spacing: Spacing.s12) {
            Text("\(Int(value))")
                .font(.minutae.mono(size: 12))
                .foregroundStyle(Color.minutae.muted)
                .frame(width: 24, alignment: .trailing)
            RoundedRectangle(cornerRadius: Radius.sm)
                .fill(Color.minutae.pine)
                .frame(width: value, height: Spacing.s8)
        }
    }
}

private struct RadiusSample: View {
    var title: String
    var radius: CGFloat

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s8) {
            RoundedRectangle(cornerRadius: radius)
                .fill(Color.minutae.mint)
                .frame(width: 72, height: Spacing.s44)
                .overlay {
                    RoundedRectangle(cornerRadius: radius)
                        .strokeBorder(Color.minutae.line, lineWidth: 1)
                }
            Text(title)
                .font(.minutae.mono(size: 12))
                .foregroundStyle(Color.minutae.muted)
        }
    }
}

struct GalleryButtons: View {
    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s12) {
            SectionLabel("Buttons")
            HStack(spacing: Spacing.s12) {
                PrimaryButton("Continue") {}
                SecondaryButton("Cancel") {}
                PrimaryButton("Save") {}.disabled(true)
            }
            HStack(spacing: Spacing.s12) {
                RecordButton {}
                RecordButton(isCapturing: true) {}
            }
            HStack(spacing: Spacing.s8) {
                Chip("Meeting", tone: .mint)
                Chip("Note", tone: .lilac)
                Chip("Dictation", tone: .peach)
                StatusPill("Selected", tone: .butter)
                StatusPill("Failed", tone: .red)
            }
        }
    }
}

struct GalleryFields: View {
    @State private var query = ""
    @State private var filled = "quarterly plan"
    @State private var scope = "Meetings"
    @State private var keepAudio = true

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s12) {
            SectionLabel("Fields")
            SearchField(text: $query)
            SearchField(text: $filled)
            SegmentedPills(options: ["Meetings", "Notes", "Dictations"], selection: $scope)
            HStack(spacing: Spacing.s16) {
                Toggle("Keep audio", isOn: $keepAudio)
                    .toggleStyle(.minutaePine)
                Toggle("Off", isOn: .constant(false))
                    .toggleStyle(.minutaePine)
            }
            .font(.minutae.ui(size: 14))
            .foregroundStyle(Color.minutae.ink)
            VStack(alignment: .leading, spacing: Spacing.s8) {
                ProgressTrack(value: 0.35)
                ProgressTrack(value: 1)
            }
            KeyCap("⌥Space", accessibilityText: "Option Space")
        }
        .frame(maxWidth: 420, alignment: .leading)
    }
}

struct GallerySettings: View {
    @State private var keepAudio = true

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s12) {
            SectionLabel("Settings")
            SettingsRow("Keep audio", description: "Save every dictation on this Mac.") {
                Toggle("Keep audio", isOn: $keepAudio)
                    .toggleStyle(.minutaePine)
                    .labelsHidden()
                    .accessibilityLabel("Keep audio")
            }
            SettingsRow("Hotkey", description: "Hold to dictate into the frontmost app.") {
                KeyCap("⌥Space", accessibilityText: "Option Space")
            }
        }
    }
}

struct GalleryCollection: View {
    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s12) {
            SectionLabel("Library")
            ListRow("Roadmap review", subtitle: "Today · 24 min", actionTitle: "Open") {}
            ListRow("Standup", subtitle: "Yesterday · 11 min", actionTitle: "Open") {}
        }
    }
}

struct GalleryHUDSection: View {
    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s12) {
            SectionLabel("HUD")
            HStack(spacing: Spacing.s8) {
                ForEach(HUDState.allCases) { state in
                    HUDCapsule(state)
                }
            }
            Text("The live dot is only used while audio is being captured.")
                .font(.minutae.ui(size: 13))
                .foregroundStyle(Color.minutae.muted)
        }
    }
}

struct GalleryEmpty: View {
    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s12) {
            SectionLabel("Empty")
            EmptyState(
                title: "Nothing here yet",
                message: "A meeting you record will show up in the library.",
                actionTitle: "Record a meeting"
            ) {}
        }
    }
}
