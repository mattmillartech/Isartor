# Isartor Installer Script for Windows (PowerShell)
# Fetches the latest release from GitHub and installs the correct binary

$ORG = "isartor-ai"
$REPO = "isartor"
$API_URL = "https://api.github.com/repos/$ORG/$REPO/releases/latest"
$INSTALL_DIR = "$env:LOCALAPPDATA\Isartor\bin"

Write-Host "[INFO] Fetching latest release info from GitHub..." -ForegroundColor Cyan
$release = Invoke-RestMethod -Uri $API_URL
$tag = $release.tag_name
$asset = $release.assets | Where-Object { $_.name -like "x86_64-pc-windows-msvc.zip" } | Select-Object -First 1

if (-not $asset) {
    Write-Host "[ERROR] Could not find a Windows release asset." -ForegroundColor Red
    exit 1
}

$url = $asset.browser_download_url
$tempZip = Join-Path $env:TEMP "isartor.zip"

Write-Host "[INFO] Downloading $($asset.name)..." -ForegroundColor Cyan
Invoke-WebRequest -Uri $url -OutFile $tempZip

Write-Host "[INFO] Extracting..." -ForegroundColor Cyan
Expand-Archive -Path $tempZip -DestinationPath $INSTALL_DIR -Force

$isartorExe = Join-Path $INSTALL_DIR "isartor.exe"
if (-not (Test-Path $isartorExe)) {
    Write-Host "[ERROR] isartor.exe not found after extraction." -ForegroundColor Red
    exit 1
}

# Add to PATH if not already present
$envPath = [System.Environment]::GetEnvironmentVariable("Path", "User")
if ($envPath -notlike "*$INSTALL_DIR*") {
    Write-Host "[INFO] Adding $INSTALL_DIR to User PATH..." -ForegroundColor Yellow
    [Environment]::SetEnvironmentVariable("Path", "$envPath;$INSTALL_DIR", "User")
}

Remove-Item $tempZip -Force

Write-Host "[SUCCESS] Isartor installed!" -ForegroundColor Green
Write-Host "Run: isartor --version to verify installation." -ForegroundColor Green
