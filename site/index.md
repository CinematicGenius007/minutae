# Minutae — You talk. It takes the minutes.

A meeting recorder and dictation app that runs on your computer. It writes plain Markdown. Free, open source, no account.

- Platforms: macOS 13+ · Windows 10+ · Linux x86_64
- License: MIT
- Source: <https://github.com/CinematicGenius007/minutae>
- Docs: <https://github.com/CinematicGenius007/minutae/tree/main/docs/install>
- Agent install guide: <https://minutae.cinematicgenius007.com/agents.md>

## Install

No release has been published yet. [Build from source](https://github.com/CinematicGenius007/minutae/blob/main/docs/install/source.md) to install today.

**Source, macOS / Linux (works today):**

```sh
git clone https://github.com/CinematicGenius007/minutae && cd minutae && sh scripts/install.sh --from-source
```

**Source, Windows (works today):**

```powershell
git clone https://github.com/CinematicGenius007/minutae; cd minutae; bun install; bun run build
```

**Ask your agent (it builds from source until the first release):**

```text
Install Minutae for me. Follow docs/agents/INSTALL.md in github.com/CinematicGenius007/minutae
```

**macOS / Linux (works from the first release):**

```sh
curl -fsSL https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/install.sh | sh
```

**Windows (works from the first release):**

```powershell
scoop install https://github.com/CinematicGenius007/minutae/releases/latest/download/minutae.json
```

## 00:04 — It asks first.

Another app opens your mic, one prompt appears. Nothing is recorded until you say so. No bot joins your call.

## 12:04 — Two tracks, so it knows who spoke.

Your mic and their audio are captured apart. Whisper writes both down on your machine; speakers get separated and you name them.

- `You` (`mic.wav`)
- Other participants (`system.wav`, separated by speaker)

## 32:14 — Hang up. You have a file.

Summary, decisions and action items as Markdown on your disk. Grep it, commit it, drop it into Obsidian.

```text
~/Library/Minutae/
├─ recordings/a1f3/
│  ├─ mic.wav
│  └─ system.wav
├─ summaries/
│  └─ q2-design-sync.md
├─ models/
└─ minutae-v4.db
```

## ⌥ space — Between calls, just talk.

Hold the key, speak, let go. The words land where your cursor is. Ctrl + Space on Windows and Linux.

## 32:40 — What runs, and where.

| Step | What | Where |
|---|---|---|
| detect | another app is using your mic | your machine |
| record | Core Audio · WASAPI · Pulse | your machine |
| transcribe | whisper.cpp · sherpa-onnx | your machine |
| summarize | Ollama · Codex CLI · Grok CLI · API key | you choose → |

This is the one step that can leave your computer. Pick Ollama and it doesn't. Pick a cloud model and the transcript is sent to it.

## Install Minutae.

- [Install guides](https://github.com/CinematicGenius007/minutae/tree/main/docs/install)
- [Read the source](https://github.com/CinematicGenius007/minutae)
- [Releases (none published yet)](https://github.com/CinematicGenius007/minutae/releases)
- [AI agent guide](https://minutae.cinematicgenius007.com/agents.md)
- [llms.txt](https://minutae.cinematicgenius007.com/llms.txt)
