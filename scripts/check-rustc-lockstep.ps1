# Verify plugin DLLs are rustc-lockstep with a Hachimi-Edge hachimi.dll.
#
# Rust release binaries embed panic-path strings of the form
# `/rustc/<40-hex-commit>/library/...`. Two DLLs built from the same rustc
# share that commit hash. edge-sdk's `ui_from_ptr` cast (plugin egui ↔ host
# egui) is only sound when this matches — a mismatch is the boot-crash mode
# reported against v0.1.0 (plugins 1.97.0 vs Edge v0.26.4 on 1.96.0).
#
# Usage:
#   ./scripts/check-rustc-lockstep.ps1 -HostDll path/to/hachimi.dll `
#       [-PluginDlls target/release/honse_tracker.dll,...]
#
# Exit code 0 = lockstep OK, 1 = mismatch or extraction failure.

param(
    [Parameter(Mandatory = $true)]
    [string]$HostDll,

    [string[]]$PluginDlls = @(
        "target/release/honse_tracker.dll",
        "target/release/honse_race_hud.dll",
        "target/release/honse_debug.dll"
    )
)

$ErrorActionPreference = "Stop"

function Get-RustcCommit {
    param([string]$Path)
    if (-not (Test-Path $Path)) {
        throw "File not found: $Path"
    }
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    $text = [System.Text.Encoding]::GetEncoding("ISO-8859-1").GetString($bytes)
    $m = [regex]::Match($text, 'rustc[/\\]([0-9a-f]{40})')
    if (-not $m.Success) {
        throw "No rustc commit string found in $Path (stripped binary?)"
    }
    return $m.Groups[1].Value
}

$hostCommit = Get-RustcCommit $HostDll
Write-Host "Host  $HostDll -> rustc commit $hostCommit"

$ok = $true
foreach ($dll in $PluginDlls) {
    $commit = Get-RustcCommit $dll
    if ($commit -eq $hostCommit) {
        Write-Host "OK    $dll -> $commit"
    } else {
        Write-Host "FAIL  $dll -> $commit (host is $hostCommit)"
        $ok = $false
    }
}

if (-not $ok) {
    Write-Error "rustc lockstep violated: rebuild plugins with the host's rustc (see rust-toolchain.toml)."
    exit 1
}
Write-Host "rustc lockstep OK: all binaries built with the same compiler."
