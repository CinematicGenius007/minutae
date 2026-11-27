# Minutae

Minutae is a local-first meeting recorder, transcriber and dictation app for macOS, Windows and Linux. Version 4 is in early development.

## Install

No release has been published yet. [Build from source](docs/install/source.md) to try Minutae today. The [install guides](docs/install/README.md) cover release downloads, package managers and updates for each platform.

If you're an AI agent installing or updating Minutae for someone, follow the [agent install guide](docs/agents/INSTALL.md).

## Meetings and dictation

- **Record meetings.** Start a recording yourself or let Minutae prompt when another app starts using the microphone. Minutae can save your mic and computer audio to separate WAV tracks.
- **Transcribe on-device.** Minutae runs whisper.cpp through Rust. After downloading a model, transcription works offline. You can show live captions while recording and label speakers using a separate downloadable model.
- **Summarize and ask questions.** Summaries include decisions and action items and are saved as Markdown with YAML frontmatter. Search the library by keyword, or ask a question using matching transcript excerpts. Configure Codex CLI, Ollama or Grok CLI as a transport in Settings, with optional API fallback. Use local Ollama to keep AI processing on your computer. Cloud transports can send transcript text to the service you configure.
- **Dictate into other apps.** Use a global shortcut to transcribe speech on-device and paste it into the active app. When Minutae can't paste, you can use the clipboard.

Semantic search and cloud transcription are planned. The current Library uses SQLite full-text search, and transcription runs locally.

## Your files

| Platform | Default data folder |
|---|---|
| macOS | `~/Library/Minutae/` |
| Windows | `%APPDATA%\Minutae\` |
| Linux | `$XDG_DATA_HOME/minutae/` or `~/.local/share/minutae/` |

The folder holds `minutae-v4.db`, recordings, Markdown summaries and models. In Settings you can change the recordings and summaries folders and adjust how long Minutae keeps meeting audio.

## Development

The desktop app lives in `apps/desktop` and uses Tauri 2, Rust, React and TypeScript. `libs/ui` contains the shared components and `crates/` contains the Rust services.

```sh
bun install
bun run dev                          # Desktop app
bun run --cwd apps/desktop test:e2e   # Browser tests with a mock backend
cargo test --workspace --locked
```

See [CLAUDE.md](CLAUDE.md) for the architecture and development commands, [the roadmap](docs/ROADMAP.md) for planned work, and [the release guide](docs/RELEASE.md) for maintainer setup.

## License

[MIT](LICENSE)
