# Builds Pane Setup: the release app and helper service first (the
# installer packs them), then the installer, copied to dist\.
#
#   powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

cargo build --release -p pane-app -p pane-scan-service
if ($LASTEXITCODE -ne 0) { throw "building Pane failed" }
# Microsoft's shader compiler (DXC), shipped beside pane.exe: from the
# newest Windows SDK that has the x64 one.
$dxc = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\dxcompiler.dll" -ErrorAction SilentlyContinue |
    Sort-Object { [version]$_.Directory.Parent.Name } -Descending | Select-Object -First 1
if (-not $dxc) { throw "dxcompiler.dll not found: install the Windows SDK" }
Copy-Item $dxc.FullName target\release\dxcompiler.dll -Force
"DXC $($dxc.VersionInfo.FileVersion) from $($dxc.DirectoryName)"

cargo build --release -p pane-setup
if ($LASTEXITCODE -ne 0) { throw "building the installer failed" }

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
New-Item -ItemType Directory -Force dist | Out-Null
$out = "dist\Pane-Setup-$version.exe"
Copy-Item target\release\pane-setup.exe $out -Force
$hash = (Get-FileHash $out -Algorithm SHA256).Hash
"Built $out ($([math]::Round((Get-Item $out).Length / 1MB, 1)) MB)"
"SHA-256: $hash"
