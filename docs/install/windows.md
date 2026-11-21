# Install on Windows

Windows 10 or 11, x64. Minutae uses Microsoft Edge WebView2, which Windows 10/11 already include.

## Install with Scoop (recommended)

[Scoop](https://scoop.sh) installs per user, needs no admin rights, and doesn't trigger the SmartScreen "unknown publisher" warning (Minutae isn't code-signed; that costs money).

1. Install Scoop once, if you don't have it (PowerShell, not as administrator):
   ```powershell
   Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser
   irm get.scoop.sh | iex
   ```
2. Install Minutae:
   ```powershell
   scoop install https://github.com/CinematicGenius007/minutae/releases/latest/download/minutae.json
   ```

Minutae appears in the Start menu. Scoop keeps it in `~\scoop\apps\minutae\`.

## Update

```powershell
scoop update minutae
```

Settings → General → **Check for updates** shows the same command when a new version is out. From a terminal, without installing anything:

```powershell
powershell -NoProfile -Command "irm https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/check-update.ps1 | iex"
```

## Without Scoop

Download `Minutae_<version>_x64-setup.exe` from [Releases](https://github.com/CinematicGenius007/minutae/releases). SmartScreen will say "Windows protected your PC": click **More info → Run anyway**. Each update is a new download.

## Uninstall

`scoop uninstall minutae`. Your data in `%APPDATA%\Minutae` stays; delete that folder to remove it.
