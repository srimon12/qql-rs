# QQL CLI Installer Script for Windows PowerShell
# Usage: irm https://raw.githubusercontent.com/srimon12/qql-rs/main/scripts/install.ps1 | iex

$ErrorActionPreference = "Stop"

$Repo = "srimon12/qql-rs"
$BinaryName = "qql.exe"

Write-Host "🔍 Detecting system architecture..." -ForegroundColor Cyan

$Arch = $env:PROCESSOR_ARCHITECTURE
if ($Arch -eq "AMD64" -or $Arch -eq "x64") {
    $TargetArch = "x86_64"
} elseif ($Arch -eq "ARM64") {
    $TargetArch = "aarch64"
} else {
    Write-Error "❌ Unsupported Architecture: $Arch"
}

$Target = "$TargetArch-pc-windows-msvc"
Write-Host "✨ Target platform: $Target" -ForegroundColor Cyan

if ($Target -ne "x86_64-pc-windows-msvc") {
    Write-Error "❌ Pre-built Windows binaries are currently published for x86_64-pc-windows-msvc only. For $Target, build from source using: cargo install qql-cli --locked"
}

$Version = $env:QQL_VERSION
if ([string]::IsNullOrEmpty($Version)) {
    Write-Host "📡 Fetching latest release version..." -ForegroundColor Cyan
    try {
        $Release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
        $Version = $Release.tag_name
    } catch {
        $Version = "v0.1.5"
    }
}

$TagName = $Version
$VersionNum = $Version -replace "^v", ""

$ArchiveName = "qql-$VersionNum-$Target.tar.gz"
$DownloadUrl = "https://github.com/$Repo/releases/download/$TagName/$ArchiveName"

$InstallDir = Join-Path $HOME ".qql\bin"
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir | Out-Null
}

$TempArchive = Join-Path $env:TEMP $ArchiveName
$TempExtractDir = Join-Path $env:TEMP "qql-extract"

Write-Host "📦 Downloading QQL CLI $TagName ($ArchiveName)..." -ForegroundColor Cyan
Invoke-WebRequest -Uri $DownloadUrl -OutFile $TempArchive

Write-Host "📂 Extracting binary..." -ForegroundColor Cyan
if (Test-Path $TempExtractDir) { Remove-Item -Path $TempExtractDir -Recurse -Force }
New-Item -ItemType Directory -Path $TempExtractDir | Out-Null

tar -xzf $TempArchive -C $TempExtractDir

$ExtractedBinary = Get-ChildItem -Path $TempExtractDir -Filter $BinaryName -Recurse | Select-Object -First 1
if (-not $ExtractedBinary) {
    Write-Error "❌ Could not find $BinaryName inside archive."
}

$DestinationPath = Join-Path $InstallDir $BinaryName
Copy-Item -Path $ExtractedBinary.FullName -Destination $DestinationPath -Force

Remove-Item -Path $TempArchive -Force -ErrorAction SilentlyContinue
Remove-Item -Path $TempExtractDir -Recurse -Force -ErrorAction SilentlyContinue

Write-Host "✅ Successfully installed qql.exe to $DestinationPath" -ForegroundColor Green

# Check if PATH contains install dir
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
    $env:Path = "$env:Path;$InstallDir"
    Write-Host "🎉 Added $InstallDir to your User PATH environment variable." -ForegroundColor Yellow
}

Write-Host "🚀 Try running: qql --version" -ForegroundColor Green
