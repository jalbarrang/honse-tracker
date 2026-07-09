<#
.SYNOPSIS
    Copy release-built honse-tracker plugin DLLs into the Honse game folder.

.DESCRIPTION
    Copies only the three Edge plugin DLLs into the game folder root:
      - target\release\honse_tracker.dll
      - target\release\race_hud.dll
      - target\release\debug_viewer.dll

    Does NOT deploy a core/proxy DLL (cri_mana_vpx.dll is Edge's job).
    Does NOT hot-swap or talk IPC — restart the Honse game to reload plugins.
    Does NOT launch or kill the game.

.PARAMETER GameDir
    The Honse game install folder (contains the game exe).
    Defaults to $env:HACHIMI_GAME_DIR or the standard Steam path.

.PARAMETER Build
    Run `cargo build --release` at the workspace root before copying.

.PARAMETER ConfigHint
    Print the `load_libraries` JSON snippet for hachimi/config.json and exit
    (no build, no copy).

.EXAMPLE
    .\scripts\deploy-windows.ps1 -Build

.EXAMPLE
    $env:HACHIMI_GAME_DIR = "D:\Games\UmamusumePrettyDerby"
    .\scripts\deploy-windows.ps1 -Build

.EXAMPLE
    .\scripts\deploy-windows.ps1 -ConfigHint
#>

param(
    [string]$GameDir = $(if ($env:HACHIMI_GAME_DIR) { $env:HACHIMI_GAME_DIR } else {
        "${env:ProgramFiles(x86)}\Steam\steamapps\common\UmamusumePrettyDerby"
    }),
    [switch]$Build,
    [switch]$ConfigHint
)

$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$TargetDir = Join-Path $RepoRoot "target\release"

$Plugins = @(
    @{ Name = "honse_tracker.dll"; Hint = "cargo build --release -p honse-tracker" },
    @{ Name = "race_hud.dll";      Hint = "cargo build --release -p race-hud" },
    @{ Name = "debug_viewer.dll";  Hint = "cargo build --release -p debug-viewer" }
)

function Show-ConfigHint {
    Write-Host ""
    Write-Host "Add the plugin DLLs to hachimi/config.json:" -ForegroundColor Cyan
    Write-Host @'
{
  "load_libraries": [
    "honse_tracker.dll",
    "race_hud.dll",
    "debug_viewer.dll"
  ]
}
'@
    Write-Host ""
    Write-Host "Place the DLLs in the Honse game folder root (same directory as the game exe)." -ForegroundColor DarkGray
    Write-Host "Restart the Honse game after copying — Edge loads plugins at startup only." -ForegroundColor DarkGray
}

if ($ConfigHint) {
    Show-ConfigHint
    exit 0
}

function Require-File {
    param([string]$Path, [string]$Hint)
    if (-not (Test-Path -LiteralPath $Path)) {
        Write-Error "Missing: $Path`n$Hint"
    }
}

function Copy-WithRetry {
    param(
        [string]$Source,
        [string]$Dest
    )
    $maxAttempts = 3
    for ($i = 1; $i -le $maxAttempts; $i++) {
        try {
            Copy-Item -LiteralPath $Source -Destination $Dest -Force
            return
        } catch {
            $locked = $_.Exception.Message -match "being used by another process"
            if ($locked) {
                if ($i -eq $maxAttempts) {
                    Write-Error @"
Cannot overwrite $Dest — the DLL is locked by the running Honse game.

Close the Honse game, then re-run:
  .\scripts\deploy-windows.ps1

(This script never kills the game process.)
"@
                }
                Write-Host "  Locked; retry $i/$maxAttempts after brief wait..." -ForegroundColor Yellow
                Start-Sleep -Milliseconds 400
                continue
            }
            throw
        }
    }
}

if ($Build) {
    Write-Host "Building release artifacts..." -ForegroundColor Cyan
    Push-Location $RepoRoot
    try {
        cargo build --release
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }
    finally {
        Pop-Location
    }
}

foreach ($p in $Plugins) {
    Require-File (Join-Path $TargetDir $p.Name) "Run: $($p.Hint)`nOr pass -Build"
}

$GameDir = $GameDir.TrimEnd('\')
if (-not (Test-Path -LiteralPath $GameDir -PathType Container)) {
    Write-Error @"
Game directory not found: $GameDir

Set -GameDir or env:HACHIMI_GAME_DIR to your Honse game install folder.
"@
}

Write-Host ""
Write-Host "Deploying plugin DLLs to: $GameDir" -ForegroundColor Green
Write-Host ""

foreach ($p in $Plugins) {
    $src = Join-Path $TargetDir $p.Name
    $dest = Join-Path $GameDir $p.Name
    Copy-WithRetry -Source $src -Dest $dest
    Write-Host "  $($p.Name)  ->  $($p.Name)"
}

Write-Host ""
Write-Host "Done." -ForegroundColor Green
Show-ConfigHint
Write-Host "Launch the Honse game yourself to verify (this script does not start the game)." -ForegroundColor DarkGray
