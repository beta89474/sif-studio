# SIF Studio v0.1.0 pre-install env check
# ASCII ONLY: PowerShell 5.1 reads .ps1 as ANSI/GBK by default;
# Chinese strings in double-quoted string literals become mojibake and break parsing.

#Requires -Version 5.1
$ErrorActionPreference = "Continue"
$pass = 0
$warn = 0
$fail = 0

function Write-Check {
    param([string]$status, [string]$line)
    switch ($status) {
        "PASS" { $script:pass++ }
        "WARN" { $script:warn++ }
        "FAIL" { $script:fail++ }
    }
    $color = switch ($status) {
        "PASS" { "Green" }
        "WARN" { "Yellow" }
        "FAIL" { "Red" }
    }
    Write-Host ("[{0}] " -f $status) -ForegroundColor $color -NoNewline
    Write-Host $line
}

Write-Host ""
Write-Host "===============================" -ForegroundColor Cyan
Write-Host " SIF Studio v0.1.0 pre-install environment check" -ForegroundColor Cyan
Write-Host "===============================" -ForegroundColor Cyan
Write-Host ""

# OS + arch
$os = (Get-CimInstance Win32_OperatingSystem).Caption
$arch = $env:PROCESSOR_ARCHITECTURE
$osBuild = [int](Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion").CurrentBuildNumber
if ($osBuild -ge 17763 -and $arch -eq "AMD64") {
    Write-Check "PASS" "OS: $os (Build $osBuild) / $arch  (Win10 1809+ or Win11, x64 = OK)"
} else {
    Write-Check "FAIL" "OS: $os (Build $osBuild) / $arch  (need Win10 1809+ or Win11 + x64)"
}

# WebView2
$wv2Paths = @(
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
    "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
    "HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
)
$wv2Found = $false
foreach ($p in $wv2Paths) {
    if (Test-Path $p) {
        $v = (Get-ItemProperty -Path $p).pv
        if ($v) {
            $wv2Found = $true
            Write-Check "PASS" "WebView2 Runtime: $v  (installed)"
            break
        }
    }
}
if (-not $wv2Found) {
    Write-Check "WARN" "WebView2 Runtime: NOT DETECTED  (Win11 usually has it; Win10 needs manual install https://developer.microsoft.com/microsoft-edge/webview2/)"
}

# VC++
$vcDll = "$env:WINDIR\System32\vcruntime140.dll"
$vcrDll = "$env:WINDIR\System32\vcruntime140_1.dll"
if (Test-Path $vcDll) {
    $v = (Get-Item $vcDll).VersionInfo.FileVersion
    Write-Check "PASS" "VC++ 2015+ Redistributable: vcruntime140.dll $v"
} else {
    Write-Check "WARN" "VC++ 2015+ Redistributable: vcruntime140.dll missing in System32 (Tauri is usually self-contained but worth fixing if app crashes with 0xc0000142)"
}

# Disk space
$drive = (Get-PSDrive C).Free / 1GB
if ($drive -ge 0.5) {
    Write-Check "PASS" "Disk space: C: free $([math]::Round($drive,2)) GB  (>= 0.5 GB recommended)"
} else {
    Write-Check "FAIL" "Disk space: C: free $([math]::Round($drive,2)) GB  (need at least 0.5 GB)"
}

# AppData write
$appDataDir = Join-Path $env:APPDATA "sif-studio"
$testFile = Join-Path $appDataDir ".write-test-$PID.tmp"
try {
    if (-not (Test-Path $appDataDir)) {
        New-Item -ItemType Directory -Path $appDataDir -Force | Out-Null
    }
    "ok" | Set-Content -Path $testFile -ErrorAction Stop
    Remove-Item $testFile -Force
    Write-Check "PASS" "User write perms: $appDataDir read-write (DB will live here)"
} catch {
    Write-Check "FAIL" "User write perms: $appDataDir NOT writable  (check ACL or antivirus)"
}

# Installer artifacts
$installerDir = Join-Path (Split-Path $PSScriptRoot -Parent) "src-tauri\target\release\bundle"
$nsis = Join-Path $installerDir "nsis\SIF Studio_0.1.0_x64-setup.exe"
$msi = Join-Path $installerDir "msi\SIF Studio_0.1.0_x64_en-US.msi"
if (Test-Path $nsis) {
    $info = Get-Item $nsis
    $hash = (Get-FileHash $nsis -Algorithm SHA256).Hash
    Write-Check "PASS" "NSIS installer: $($info.Length) bytes / SHA256 $hash"
} else {
    Write-Check "WARN" "NSIS installer: not found at $nsis  (only matters if you build it yourself; otherwise skip)"
}
if (Test-Path $msi) {
    $info = Get-Item $msi
    $hash = (Get-FileHash $msi -Algorithm SHA256).Hash
    Write-Check "PASS" "MSI installer: $($info.Length) bytes / SHA256 $hash"
} else {
    Write-Check "WARN" "MSI installer: not found at $msi"
}

# Already-installed check
$uninstPaths = @(
    "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\SIF Studio_is1",
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\SIF Studio_is1"
)
$installed = $false
foreach ($p in $uninstPaths) {
    if (Test-Path $p) {
        $v = (Get-ItemProperty -Path $p).DisplayVersion
        $installed = $true
        Write-Check "WARN" "Already installed: v$v  (uninstall first to avoid NSIS upgrade vs clean install surprises)"
        break
    }
}
if (-not $installed) {
    Write-Check "PASS" "SIF Studio: not installed"
}

Write-Host ""
Write-Host "====== Summary ======" -ForegroundColor Cyan
Write-Host "PASS : $pass" -ForegroundColor Green
Write-Host "WARN : $warn" -ForegroundColor Yellow
Write-Host "FAIL : $fail" -ForegroundColor Red

if ($fail -gt 0) {
    Write-Host ""
    Write-Host "[FAIL] $fail failure(s) - fix before installing" -ForegroundColor Red
    exit 1
} elseif ($warn -gt 0) {
    Write-Host ""
    Write-Host "[WARN] $warn warning(s) - usually ignorable; double-check red lines above" -ForegroundColor Yellow
    exit 0
} else {
    Write-Host ""
    Write-Host "[OK] All PASS - safe to double-click installer" -ForegroundColor Green
    exit 0
}
