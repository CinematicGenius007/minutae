# SPIKE-01 — native Swift feasibility (throwaway)

Delete this entire `spikes/swift/` tree when the verdict is done.

## Build

```bash
cd spikes/swift
swift build --product spike-cli
swift build --product MinutaeSpike
./Scripts/package-app.sh   # ad-hoc signed MinutaeSpike.app under AppBundle/
```

## Run probes

```bash
BIN=$(swift build --product spike-cli --show-bin-path)/spike-cli
"$BIN" speech whisper-cpp --seconds 10
"$BIN" speech whisperkit --seconds 10
"$BIN" llm grok
MINUTAE_CODEX_MODEL='…' "$BIN" llm codex
"$BIN" llm cancel-demo
"$BIN" process-tap --pid "$(pgrep -x Music | head -1)"
"$BIN" insertion ax
"$BIN" secure-check
"$BIN" package-info
```

GUI (hotkey + overlay + menu bar):

```bash
open AppBundle/MinutaeSpike.app
# or
SPIKE_AUTO_DEMO=1 ./AppBundle/MinutaeSpike.app/Contents/MacOS/MinutaeSpike
```

Hold ⌥Space anywhere. Overlay should show without stealing focus.

## Results

See [RESULTS.md](./RESULTS.md).
