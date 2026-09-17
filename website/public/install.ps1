# QQL CLI Installer Script for Windows PowerShell
# Usage:
#   irm https://qql.veristamp.in/install.ps1 | iex
#   & ([scriptblock]::Create((irm https://qql.veristamp.in/install.ps1))) -Full
#   & ([scriptblock]::Create((irm https://qql.veristamp.in/install.ps1))) -Version 0.4.0

param(
    [string]$Version = $env:QQL_VERSION,
    [string]$Edition = $env:QQL_EDITION,
    [switch]$Full = ($env:QQL_EDITION -eq "full"),
    [string]$InstallDir = $env:QQL_INSTALL_DIR
)

$ErrorActionPreference = "Stop"

$Repo = "srimon12/qql-rs"
$BinaryName = "qql.exe"

if ($args -contains "--full" -or $args -contains "-full" -or $Edition -eq "full") {
    $Full = $true
}
$Edition = if ($Full) { "full" } else { "standard" }

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

if ([string]::IsNullOrEmpty($Version)) {
    Write-Host "📡 Fetching latest release version..." -ForegroundColor Cyan
    try {
        $req = [System.Net.WebRequest]::Create("https://github.com/$Repo/releases/latest")
        $req.Method = "HEAD"
        $req.AllowAutoRedirect = $false
        $resp = $req.GetResponse()
        $location = $resp.GetResponseHeader("Location")
        $resp.Close()
        if ($location -match "/tag/v?([^/]+)$") {
            $Version = $Matches[1]
        }
    } catch {
        # Fallback to GitHub REST API if HEAD request fails
        try {
            $Release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
            $Version = $Release.tag_name
        } catch {
            Write-Error "❌ Could not determine the latest release version from GitHub. Set `$env:QQL_VERSION explicitly (e.g. `$env:QQL_VERSION='0.4.0') and retry."
        }
    }
}

if ([string]::IsNullOrEmpty($Version)) {
    Write-Error "❌ GitHub returned an empty release tag. Set `$env:QQL_VERSION explicitly and retry."
}

$VersionNum = $Version -replace "^v", ""
$TagName = "v$VersionNum"

$Prefix = if ($Full) { "qql-full" } else { "qql" }
$ArchiveName = "$Prefix-$VersionNum-$Target.tar.gz"
$DownloadUrl = "https://github.com/$Repo/releases/download/$TagName/$ArchiveName"

if ([string]::IsNullOrEmpty($InstallDir)) {
    $InstallDir = Join-Path $HOME ".qql\bin"
}
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

$TempArchive = Join-Path $env:TEMP $ArchiveName
$TempExtractDir = Join-Path $env:TEMP "qql-extract-$(Get-Random)"

Write-Host "📦 Downloading QQL $Edition edition ($ArchiveName)..." -ForegroundColor Cyan
Invoke-WebRequest -Uri $DownloadUrl -OutFile $TempArchive

Write-Host "📂 Extracting binary..." -ForegroundColor Cyan
if (Test-Path $TempExtractDir) { Remove-Item -Path $TempExtractDir -Recurse -Force }
New-Item -ItemType Directory -Path $TempExtractDir -Force | Out-Null

tar -xzf $TempArchive -C $TempExtractDir

$ExtractedBinary = Get-ChildItem -Path $TempExtractDir -Filter $BinaryName -Recurse | Select-Object -First 1
if (-not $ExtractedBinary) {
    Write-Error "❌ Could not find $BinaryName inside archive."
}

$DestinationPath = Join-Path $InstallDir $BinaryName
Copy-Item -Path $ExtractedBinary.FullName -Destination $DestinationPath -Force

Remove-Item -Path $TempArchive -Force -ErrorAction SilentlyContinue
Remove-Item -Path $TempExtractDir -Recurse -Force -ErrorAction SilentlyContinue

Write-Host "✅ Successfully installed qql ($Edition edition) to $DestinationPath" -ForegroundColor Green

# Check if PATH contains install dir
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
    $env:Path = "$env:Path;$InstallDir"
    Write-Host "🎉 Added $InstallDir to your User PATH environment variable." -ForegroundColor Yellow
}

Write-Host "🚀 Try running: qql version" -ForegroundColor Green
