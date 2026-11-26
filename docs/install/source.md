# Build from source

You can build Minutae without a published app release. The first build compiles native dependencies and can take several minutes.

## Tools

| | macOS | Windows | Linux |
|---|---|---|---|
| Rust (stable) | [rustup.rs](https://rustup.rs) | rustup.rs | rustup.rs |
| Bun 1.4+ | [bun.sh](https://bun.sh) | bun.sh | bun.sh |
| cmake | `brew install cmake` | Visual Studio Build Tools (C++ workload) includes it | distro package |
| Other | Xcode Command Line Tools: `xcode-select --install` | LLVM (`winget install LLVM.LLVM`), then set `LIBCLANG_PATH=C:\Program Files\LLVM\bin` | packages below |

Linux (Debian/Ubuntu names):

```sh
sudo apt install build-essential cmake clang libclang-dev pkg-config patchelf file \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev \
  libasound2-dev libpulse-dev libxdo-dev libdbus-1-dev libssl-dev \
  libxkbcommon-dev libx11-dev libxcb1-dev libxi-dev libxtst-dev
```

## macOS and Linux

```sh
git clone https://github.com/CinematicGenius007/minutae.git
cd minutae
sh scripts/install.sh --from-source
```

That runs `bun install`, `bun run tauri build`, and installs the result like a release (`/Applications` or `~/Applications` on macOS, `~/.local/bin/minutae` on Linux). Run outside a checkout, it clones to `~/.local/src/minutae` (`MINUTAE_SRC` to change).

Update: `git pull && sh scripts/install.sh --from-source`. Settings → Updates shows this command when a new release is out.

On macOS, local builds are signed ad hoc, so macOS asks for the microphone and other permissions again after each rebuild.

## Windows

```powershell
git clone https://github.com/CinematicGenius007/minutae.git
cd minutae
bun install
bun run build
```

The installer is in `target\release\bundle\nsis\`, and the plain app is `target\release\minutae-desktop.exe`. Update: `git pull; bun install; bun run build`.

## Developing

See [CLAUDE.md](../../CLAUDE.md) for the layout, commands and rules (`bun run dev` runs the app with hot reload).
