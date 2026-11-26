# Install on Windows

Needs Windows 10 or 11 (x64) and Microsoft Edge WebView2. The setup EXE uses Microsoft's bootstrapper if WebView2 needs to be installed.

Release installs become available once the first release is published. For now, see [Build from source](source.md).

## 1. Install with Scoop

If you already have [Scoop](https://scoop.sh), run this from your normal PowerShell window:

```powershell
scoop install https://github.com/CinematicGenius007/minutae/releases/latest/download/minutae.json
```

Scoop installs for your user and adds Minutae to the Start menu. To update, quit Minutae and run `scoop update minutae`. Settings → General → **Check for updates** shows the same command when an update is available. To remove the app, run `scoop uninstall minutae`.

## 2. Install with PowerShell

Run this in a normal PowerShell window:

```powershell
irm https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/install.ps1 | iex
```

The script uses Scoop if it is already installed, unless you're updating an existing setup EXE install. Otherwise it downloads the latest release's setup EXE, checks it against `SHA256SUMS.txt` and runs the per-user installer without elevation. The script leaves PowerShell's execution policy alone. PowerShell downloads normally avoid the browser's Mark-of-the-Web, but Windows 11 Smart App Control can still block unsigned installers or apps, including with Scoop. [Microsoft's Smart App Control documentation](https://learn.microsoft.com/en-us/windows/security/application-security/application-control/smart-app-control)

To update a Minutae copy installed by the setup EXE, use **Install and restart** under **Settings → General → Check for updates** if the release includes updater signatures. Otherwise, quit Minutae and rerun the same PowerShell command. You can also get `Minutae_<version>_x64-setup.exe` and its checksum directly from [Releases](https://github.com/CinematicGenius007/minutae/releases).

For setup EXE installs, uninstall from **Settings → Apps**. Both install routes keep your data in `%APPDATA%\Minutae` by default.
