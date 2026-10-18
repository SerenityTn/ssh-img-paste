<#
.SYNOPSIS
Builds the Windows installer and portable zip for SSH Image Paste.

.DESCRIPTION
Builds the Rust CLI, publishes the self-contained WPF app with the CLI beside
it, then compiles the Inno Setup installer. Outputs land in windows/artifacts.

.PARAMETER Iscc
Path to ISCC.exe. Defaults to the ISCC environment variable, then the usual
Inno Setup 6 install folders.

.PARAMETER SkipCli
Reuse an existing release CLI build instead of running cargo.
#>
[CmdletBinding()]
param(
    [string] $Iscc = $env:ISCC,
    [switch] $SkipCli
)

$ErrorActionPreference = "Stop"
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$project = Join-Path $repo "windows\SSHImagePaste.Windows\SSHImagePaste.Windows.csproj"
$cli = Join-Path $repo "rust\ssh-img-paste-cli\target\release\ssh-img-paste.exe"
$artifacts = Join-Path $repo "windows\artifacts"
$publish = Join-Path $artifacts "publish\win-x64"

$version = ([xml](Get-Content $project)).Project.PropertyGroup.Version | Where-Object { $_ } | Select-Object -First 1
if (-not $version) { throw "No <Version> in $project" }
Write-Host "SSH Image Paste $version"

if (-not $SkipCli) {
    cargo build --release --locked --manifest-path (Join-Path $repo "rust\ssh-img-paste-cli\Cargo.toml")
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}
if (-not (Test-Path $cli)) { throw "Missing CLI build: $cli" }

if (Test-Path $publish) { Remove-Item -Recurse -Force $publish }
dotnet publish $project -c Release -r win-x64 --self-contained -o $publish "-p:SshImgPasteCli=$cli"
if ($LASTEXITCODE -ne 0) { throw "dotnet publish failed" }
foreach ($required in "SSHImagePaste.exe", "ssh-img-paste.exe") {
    if (-not (Test-Path (Join-Path $publish $required))) { throw "Publish output is missing $required" }
}

$zip = Join-Path $artifacts "SSHImagePaste-$version-x64-portable.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path (Join-Path $publish "*") -DestinationPath $zip -CompressionLevel Optimal

if (-not $Iscc) {
    $Iscc = @(
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $Iscc -or -not (Test-Path $Iscc)) { throw "ISCC.exe not found. Install Inno Setup 6 or pass -Iscc." }

& $Iscc /Q "/DAppVersion=$version" "/DSourceDir=$publish" "/DOutputDir=$artifacts" (Join-Path $PSScriptRoot "SSHImagePaste.iss")
if ($LASTEXITCODE -ne 0) { throw "Inno Setup compilation failed" }

Get-ChildItem $artifacts -File | Where-Object { $_.Name -like "SSHImagePaste-*" } |
    ForEach-Object { "{0}  {1:N1} MB" -f $_.FullName, ($_.Length / 1MB) }
