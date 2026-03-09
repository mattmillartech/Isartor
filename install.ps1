$ErrorActionPreference = "Stop"

$Repo = "isartor-ai/Isartor"
$BinName = "isartor.exe"
$ArtifactName = "isartor-windows-amd64.exe"
$InstallDir = "$env:USERPROFILE\.local\bin"

Write-Host "Installing $BinName..."

# Fetch latest release data
$LatestReleaseUrl = "https://api.github.com/repos/$Repo/releases/latest"
Write-Host "Fetching latest release information..."
$ReleaseData = Invoke-RestMethod -Uri $LatestReleaseUrl

$DownloadUrl = $null
foreach ($Asset in $ReleaseData.assets) {
    if ($Asset.name -eq $ArtifactName) {
        $DownloadUrl = $Asset.browser_download_url
        break
    }
}

if ($null -eq $DownloadUrl) {
    Write-Error "Could not find a release artifact for Windows amd64"
    exit 1
}

if (-Not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
}

$DestPath = "$InstallDir\$BinName"
Write-Host "Downloading from $DownloadUrl..."
Invoke-WebRequest -Uri $DownloadUrl -OutFile $DestPath

Write-Host "✅ $BinName installed to $InstallDir!"

# Check if it's in path, if not add it
$Path = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($Path -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("PATH", "$Path;$InstallDir", "User")
    $env:PATH = "$env:PATH;$InstallDir"
    Write-Host "Added $InstallDir to your PATH."
}

Write-Host "Installation complete! Run 'isartor' to start."
