# Builds openmapper.exe and a zip of it in target\package (Windows; -Target aarch64-pc-windows-msvc
# for ARM64, the program's icon is built into the .exe by build.rs): the program, the README
# and the tutorial (in docs, where the README links it). The C runtime is linked in
# (.cargo\config.toml): nothing to install.
param([string]$Target = "")
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $Target) { $Target = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "aarch64-pc-windows-msvc" } else { "x86_64-pc-windows-msvc" } }
$arch = if ($Target -like "aarch64*") { "arm64" } else { "x64" }
cargo build --release --locked --target $Target
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$out = "target\package"
$name = "openMapper-$version-windows-$arch"
$dir = Join-Path $out $name
if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
New-Item -ItemType Directory -Force (Join-Path $dir "docs") | Out-Null
Copy-Item "target\$Target\release\openmapper.exe", README.md $dir
Copy-Item docs\TUTORIAL.md (Join-Path $dir "docs")
if (Test-Path LICENSE) { Copy-Item LICENSE $dir }
$zip = Join-Path $out "$name.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path $dir -DestinationPath $zip
Write-Output $zip
