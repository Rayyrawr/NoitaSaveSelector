# Builds the release exe and the installer into dist\.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location "$root\app"
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
    $version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"(.+)"').Matches[0].Groups[1].Value
} finally { Pop-Location }

$iscc = @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup 6 not found (winget install JRSoftware.InnoSetup)' }

& $iscc "/DAppVersion=$version" "$PSScriptRoot\NoitaSaveSelector.iss"
if ($LASTEXITCODE -ne 0) { throw 'ISCC failed' }

# Release assets: installer, plain exe (used by the in-app updater) and its checksum.
$dist = "$root\dist"
Copy-Item "$root\app\target\release\NoitaSaveSelector.exe" $dist -Force
$sha = [System.Security.Cryptography.SHA256]::Create()
$hash = -join ($sha.ComputeHash([System.IO.File]::ReadAllBytes("$dist\NoitaSaveSelector.exe")) | ForEach-Object { $_.ToString('x2') })
"$hash  NoitaSaveSelector.exe" | Set-Content -NoNewline -Encoding ascii "$dist\NoitaSaveSelector.exe.sha256"
Write-Host "Fertig: dist\NoitaSaveSelector-Setup-$version.exe (v$version)"
