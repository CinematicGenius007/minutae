# Is there a newer Minutae? Windows version of check-update.sh. Read-only.
#
#   powershell -NoProfile -Command "irm https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/check-update.ps1 | iex"
#   pwsh scripts/check-update.ps1 [-Current 0.4.0] [-Json]
# (Run it in a child `powershell` as above: the script ends with `exit`, which would close
# your own window if you piped it into `iex` directly.)
#
# Exit code: 0 up to date, 10 update available, 1 couldn't tell.
# Finds the installed copy in Scoop (~\scoop\apps\minutae\current) or the NSIS install
# (%LOCALAPPDATA%\Minutae\Minutae.exe), else the checkout this script sits in.
param([string]$Current = "", [switch]$Json)
$ErrorActionPreference = "Stop"
$Repo = "CinematicGenius007/minutae"
$Channel = if ($Current) { "given" } else { "" }

function Done($code, $obj, $text) {
  if ($Json) { $obj | ConvertTo-Json -Compress } else { $text | ForEach-Object { Write-Output $_ } }
  exit $code
}
function Fail($msg) { Done 1 @{ status = "unknown"; error = $msg } @($msg) }

if (-not $Current) {
  $scoopRoot = if ($env:SCOOP) { $env:SCOOP } else { Join-Path $HOME "scoop" }
  $manifest = Join-Path $scoopRoot "apps\minutae\current\manifest.json"
  if (Test-Path $manifest) {
    $Current = (Get-Content $manifest -Raw | ConvertFrom-Json).version
    $Channel = "scoop"
  }
}
if (-not $Current) {
  $exe = Join-Path $env:LOCALAPPDATA "Minutae\Minutae.exe"
  if (Test-Path $exe) {
    $Current = (Get-Item $exe).VersionInfo.ProductVersion
    $Channel = "direct"
  }
}
$Src = $null
if (-not $Current -and $PSScriptRoot) {
  $cargo = Join-Path $PSScriptRoot "..\Cargo.toml"
  if (Test-Path $cargo) {
    $m = Select-String -Path $cargo -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if ($m) { $Current = $m.Matches[0].Groups[1].Value; $Channel = "source"; $Src = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path }
  }
}
if (-not $Current) { Fail "Minutae doesn't look installed here. Pass -Current <version> to compare anyway." }
$Current = $Current.TrimStart("v")

try {
  $r = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ Accept = "application/vnd.github+json" } -TimeoutSec 20
} catch {
  $code = $_.Exception.Response.StatusCode.value__
  if ($code -eq 404) { Fail "No published release yet." }
  if ($code -eq 403 -or $code -eq 429) { Fail "GitHub is limiting requests. Try again in an hour." }
  Fail "Couldn't reach GitHub."
}
$Latest = "$($r.tag_name)".TrimStart("v")

function Parse($v) {
  $parts = $v -split "-", 2
  [pscustomobject]@{ Num = [version]$parts[0]; Pre = ($parts.Count -gt 1) }
}
$a = Parse $Current; $b = Parse $Latest
$isNewer = ($b.Num -gt $a.Num) -or ($b.Num -eq $a.Num -and $a.Pre -and -not $b.Pre)

$Cmd = switch ($Channel) {
  "scoop"  { "scoop update minutae" }
  "source" { "cd `"$Src`"; git pull; bun install; bun run build" }
  default  { "scoop install https://github.com/$Repo/releases/latest/download/minutae.json" }
}

if ($isNewer) {
  Done 10 @{ status = "available"; current = $Current; latest = $Latest; channel = $Channel; command = $Cmd } @(
    "Minutae $Latest is available (you have $Current).",
    "Update with: $Cmd",
    "Notes: https://github.com/$Repo/releases/tag/v$Latest"
  )
}
Done 0 @{ status = "upToDate"; current = $Current; latest = $Latest; channel = $Channel } @("Minutae $Current is up to date (latest: $Latest).")
