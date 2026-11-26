# Install or update Minutae for the current user from Windows PowerShell or PowerShell 7.
#   irm https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/install.ps1 | iex
#
# Prefer an existing Scoop installation. Otherwise verify the NSIS installer against
# SHA256SUMS.txt and run it. Quit Minutae before updating.
& {
    $ErrorActionPreference = 'Stop'
    if ($env:OS -ne 'Windows_NT') { throw 'This installer is for Windows.' }

    $repo = 'CinematicGenius007/minutae'
    $scoop = Get-Command scoop -CommandType Application, ExternalScript -ErrorAction SilentlyContinue
    $scoopRoot = if ($env:SCOOP) { $env:SCOOP } else { Join-Path $HOME 'scoop' }
    $scoopManifest = Join-Path $scoopRoot 'apps\minutae\current\manifest.json'
    $nsisInstalled = Test-Path -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Minutae'
    if ($scoop -and ((Test-Path -LiteralPath $scoopManifest) -or -not $nsisInstalled)) {
        if (Test-Path -LiteralPath $scoopManifest) {
            & $scoop update minutae
        } else {
            & $scoop install "https://github.com/$repo/releases/latest/download/minutae.json"
        }
        return
    }

    try {
        $release = Invoke-RestMethod -UseBasicParsing -TimeoutSec 30 `
            -Uri "https://api.github.com/repos/$repo/releases/latest"
    } catch {
        if ($_.Exception.Response.StatusCode -eq 404) { throw 'No published release yet.' }
        throw
    }
    if ($release.tag_name -cnotmatch '^v(\d+\.\d+\.\d+)$') {
        throw 'GitHub did not return a stable Minutae release tag.'
    }
    $version = $Matches[1]
    $file = "Minutae_${version}_x64-setup.exe"
    $baseUrl = "https://github.com/$repo/releases/download/v$version"
    $temp = Join-Path ([System.IO.Path]::GetTempPath()) "minutae-$([guid]::NewGuid())"
    New-Item -ItemType Directory -Path $temp | Out-Null
    try {
        $checksums = Join-Path $temp 'SHA256SUMS.txt'
        $installer = Join-Path $temp $file
        Invoke-WebRequest -UseBasicParsing -Uri "$baseUrl/SHA256SUMS.txt" -OutFile $checksums
        $pattern = '^([a-fA-F0-9]{64}) [ *]' + [regex]::Escape($file) + '$'
        $entry = @(Select-String -LiteralPath $checksums -Pattern $pattern -CaseSensitive)
        if ($entry.Count -ne 1) { throw "Missing or ambiguous checksum for $file." }

        Invoke-WebRequest -UseBasicParsing -Uri "$baseUrl/$file" -OutFile $installer
        $actual = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash
        if ($actual -ne $entry[0].Matches[0].Groups[1].Value) { throw "Checksum mismatch for $file." }

        $process = Start-Process -FilePath $installer -Wait -PassThru
        if ($process.ExitCode -ne 0) { throw "Minutae setup exited with code $($process.ExitCode)." }
    } finally {
        Remove-Item -LiteralPath $temp -Recurse -Force
    }
}
