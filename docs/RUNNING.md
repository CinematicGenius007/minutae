# Running Minutae

The Xcode app is a debug harness. It opens the real pipeline: microphone, Whisper, jobs, and the Codex, Grok, and Ollama transports. The window is a light, plain SwiftUI list. It is not the designed product UI.

## Build and launch

1. Open `Minutae.xcodeproj` and select the **Minutae** scheme.
2. Sign with a free Apple Development certificate. Steps are in [signing.md](signing.md). Ad-hoc signing resets Microphone and Accessibility every rebuild.
3. Run. The app is not sandboxed.
4. Data lands in `~/Library/Minutae/`:
   - `minutae.db` — meetings, dictations, jobs
   - `recordings/` — meeting WAV files
   - `summaries/` — markdown summaries
   - `dictations/YYYY-MM/` — dictation WAV files
   - `models/` — Whisper GGML files (`ggml-base.bin` by default)

A missing model, a missing CLI, or a denied permission is shown as text in the window. Those states do not quit the app.

## Permissions

In the window, use **Request** for Microphone and Accessibility. Screen Recording and Calendars are listed too.

- Microphone denied: recording and dictation stay off.
- Accessibility denied: dictated text is copied to the clipboard instead of inserted. The HUD says so.
- Screen Recording denied: meetings keep the microphone only. Process-tap audio is not in this harness.
- Calendars denied: meetings have no calendar titles. Calendar naming is not wired in this harness.

If macOS will not ask again, use **Settings** on that row, or System Settings → Privacy & Security.

## Whisper model

The default model name is `base`. The harness shows whether it is installed and loaded.

Click **Download or load model**. The file comes from the official `ggerganov/whisper.cpp` Hugging Face repository and is checked before it is kept. Until it is loaded, a meeting can still be recorded. Transcription then fails with a message that names the missing model, and the job row keeps that error.

## AI without an API key

The fallback order is Codex, then Grok, then Ollama. The API transport is not in this build.

1. Install the tool you want and sign in with **its own** CLI. Minutae does not read credential files.
   - Codex: install `codex`, run `codex login`, then type the model id in **Codex model** and click **Save**. An empty model makes Test say to select one.
   - Grok: install `grok` and sign in with `grok`.
   - Ollama: install Ollama, run `ollama serve`, and `ollama pull` a small local model. A cloud-only model is reported as not ready.
2. Click **Refresh status**. Each row shows installed or not, and signed-in when the transport can say.
3. Click **Test** on a row. A missing binary, a signed-out CLI, or a rejected model is the text under that row.

Summary jobs use the same router. A failed summary leaves the meeting status and the job error visible in the lists.

## Dictation

The hotkey is hold Option-Space (`⌥Space`). Hold it in another app, speak, and release.

A small panel at the bottom of the screen shows Listening, Processing, Inserting, or the error. It does not take keyboard focus. The same words appear in the harness next to **Cancel dictation**.

Every attempt is a row: time, text, insert method (`pasteboard`, `accessibility`, `typing`, `clipboardOnly`, or `none`), status, and the WAV path. Secure fields are refused and the row says so.

Option-Space is registered with Carbon. It does not need Input Monitoring. If registration fails, the window shows the failure and a suggested fallback chord. Change the chord later in settings; this harness does not draw a hotkey picker.

## Record a meeting

1. Grant the microphone.
2. Load a Whisper model if you want a transcript.
3. Click **Record meeting**. The elapsed time runs beside **Stop**.
4. Click **Stop**. The WAV is closed, a transcribe job is queued, and a successful transcript queues a summary job.
5. The meeting list shows the title, status, and summary path. **Open in Finder** reveals the markdown file.

The summary is `~/Library/Minutae/summaries/DATE-title.md` with YAML frontmatter. The database copy is the `summaries` table.

One microphone is shared. Starting a meeting while dictation is listening, or the reverse, fails with a message that the microphone is already in use.

## Fake smoke run

Launch with `--fake` or `MINUTAE_FAKES=1` to swap in fake audio, fake speech, a fake hotkey, a fake inserter, and fake LLM replies. No microphone, model, or CLI is required. Files go to `~/Library/Minutae/fake/` instead of the real database.

```bash
# From an Xcode run: Product → Scheme → Edit Scheme → Run → Arguments
# Arguments Passed On Launch: --fake
# or an environment variable: MINUTAE_FAKES=1
```

The window says **Debug harness · fakes**.

## Reset

Quit Minutae, then delete the folder you want to clear:

```bash
rm -rf ~/Library/Minutae
```

That removes the database, recordings, summaries, dictations, models, and the fake-smoke folder. The next launch creates an empty database. Settings for the real app live in the UserDefaults suite `com.nebula.minutae`, not in that folder. To reset those as well:

```bash
defaults delete com.nebula.minutae
```

Deleting `~/Library/Minutae` does not sign you out of Codex, Grok, or Ollama.

## Tests

```bash
xcodebuild build test -project Minutae.xcodeproj -scheme Minutae \
  CODE_SIGNING_ALLOWED=NO -destination 'platform=macOS'
```

The app test target checks that the composition root boots on fakes, and that a fake recording becomes a markdown summary and a finished dictation row. The test host does not open `~/Library/Minutae`.
