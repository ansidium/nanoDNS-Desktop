# Build and verify an unsigned Windows x64 release. Never signs or publishes.
[CmdletBinding()]
param(
    [string]$OutputDirectory = (Join-Path $PSScriptRoot 'dist'),
    [string]$Python = 'python'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($env:OS -ne 'Windows_NT') {
    throw 'This application must be built on Windows with the MSVC toolchain.'
}

function Invoke-Checked {
    param([string]$Command, [string[]]$CommandArguments)
    Write-Host ('> {0} {1}' -f $Command, ($CommandArguments -join ' '))
    & $Command @CommandArguments
    if ($LASTEXITCODE -ne 0) {
        throw ('{0} exited with code {1}.' -f $Command, $LASTEXITCODE)
    }
}

$toolchain = '+1.99.1'
$target = 'x86_64-pc-windows-msvc'
Push-Location -LiteralPath $PSScriptRoot
try {
    Invoke-Checked $Python @('scripts/check-locales.py')
    Invoke-Checked 'cargo' @($toolchain, 'fmt', '--all', '--', '--check')
    Invoke-Checked 'cargo' @($toolchain, 'clippy', '--all-targets', '--locked', '--target', $target, '--', '-D', 'warnings')
    Invoke-Checked 'cargo' @($toolchain, 'test', '--all-targets', '--locked', '--target', $target, '--', '--test-threads=1')
    Invoke-Checked 'cargo' @($toolchain, 'build', '--release', '--locked', '--target', $target)

    $targetDirectory = if ([string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
        Join-Path $PSScriptRoot 'target'
    } else {
        [System.IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
    }
    $executable = Join-Path $targetDirectory "$target/release/nanodns-desktop.exe"
    Invoke-Checked $Python @('verify-release.py', $executable)
    Invoke-Checked $Python @('scripts/package-release.py', '--exe', $executable, '--output', $OutputDirectory)
} finally {
    Pop-Location
}
