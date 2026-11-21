# Brand assets

Source of truth for every Minutae icon. Design reference: Paper file (see CLAUDE.md), page "Minutae — Brand & icons".

The mark is a geometric lowercase **m** plus the round dot from the "minutae ●" wordmark. It uses ink and white only. Red (`--onair`) means recording and is never part of the brand (DESIGN-LANGUAGE rule 2).

| File | Use |
|---|---|
| `app-icon-macos.svg` | macOS app icon: 824 px rounded rect on a transparent 1024 canvas (→ `icon.icns`) |
| `app-icon-square.svg` | Windows/Linux icon: full-bleed tile (→ `.ico` and PNGs) |
| `favicon.svg` | UI and gallery favicon |
| `tray-idle.svg`, `tray-recording.svg` | Menu-bar/tray template images (alpha only) |
| `mark.svg` | Mark on its own |

Regenerate:

```bash
python3 assets/brand/generate.py                     # SVGs
cd apps/desktop
bun run tauri icon ../../assets/brand/app-icon-square.svg   # all icons
bun run tauri icon ../../assets/brand/app-icon-macos.svg -o /tmp/mac && cp /tmp/mac/icon.icns src-tauri/icons/
bun run tauri icon ../../assets/brand/tray-idle.svg -o /tmp/ti --png 64 && cp /tmp/ti/64x64.png src-tauri/icons/tray/idle.png
bun run tauri icon ../../assets/brand/tray-recording.svg -o /tmp/tr --png 64 && cp /tmp/tr/64x64.png src-tauri/icons/tray/recording.png
cp ../../assets/brand/favicon.svg public/ && cp ../../assets/brand/favicon.svg ../../libs/ui/dev/public/
```

`tauri icon` on the square SVG also writes `android/` and `ios/` folders; delete them, they are not used.
