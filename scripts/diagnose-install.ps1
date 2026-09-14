# SIF Studio v0.1.0 install diagnostic
# Why window does not appear after install
# Run:  powershell -ExecutionPolicy Bypass -File diagnose-install.ps1
# PURE ASCII ONLY: PowerShell 5.1 reads .ps1 as ANSI/GBK by default;
# Chinese strings in double-quoted string literals become mojibake and break parsing.
# All visible labels are ASCII; the report file (UTF-8) is the user-facing artifact.

#Requires -Version 5.1
$ErrorActionPreference = "Continue"
$out = New-Object System.Collections.Generic.List[string]

function Add {
    param([string]$label, [string]$value)
    $line = "[{0}] {1}" -f $label, $value
    Write-Host $line
    $out.Add($line) | Out-Null
}

function Section {
    param([string]$name)
    $line = "===== $name ====="
    Write-Host ""
    Write-Host $line -ForegroundColor Cyan
    $out.Add("") | Out-Null
    $out.Add($line) | Out-Null
}

Section "Environment"
Add "Time"   (Get-Date -Format "yyyy-MM-dd HH:mm:ss zzz")
Add "Machine" $env:COMPUTERNAME
Add "User"   "$env:USERDOMAIN\$env:USERNAME"
Add "OS"     (Get-CimInstance Win32_OperatingSystem | Select-Object -ExpandProperty Caption)
Add "Build"  ([int](Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion").CurrentBuildNumber)
Add "Arch"   $env:PROCESSOR_ARCHITECTURE

Section "WebView2 Runtime (most common cause: missing -> app starts and dies silently)"
$wv2Paths = @(
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
    "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
    "HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
)
$found = $false
foreach ($p in $wv2Paths) {
    if (Test-Path $p) {
        $pv = (Get-ItemProperty -Path $p).pv
        $bp = (Get-ItemProperty -Path $p).branding
        Add "WebView2" "installed version=$pv branding=$bp"
        $found = $true
        break
    }
}
if (-not $found) {
    Add "WebView2" "NOT INSTALLED  <-- install: https://developer.microsoft.com/microsoft-edge/webview2/"
}

Section "VC++ 2015+ x64 Redistributable"
$vcKey = "HKLM:\SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64"
if (Test-Path $vcKey) {
    $v = (Get-ItemProperty -Path $vcKey).Version
    Add "VC++ x64 Redist" "installed v=$v"
} else {
    Add "VC++ x64 Redist" "NOT INSTALLED  <-- install: https://aka.ms/vs/17/release/vc_redist.x64.exe"
}
Add "vcruntime140.dll"   (Test-Path "$env:WINDIR\System32\vcruntime140.dll")
Add "vcruntime140_1.dll" (Test-Path "$env:WINDIR\System32\vcruntime140_1.dll")

Section "SIF Studio install location"
$candidates = @(
    "$env:LOCALAPPDATA\Programs\SIF Studio",
    "$env:LOCALAPPDATA\Programs\SIF Studio\sif-studio.exe",
    "${env:ProgramFiles}\SIF Studio",
    "${env:ProgramFiles(x86)}\SIF Studio"
)
foreach ($p in $candidates) {
    if (Test-Path $p) {
        Add "found" $p
    }
}

$uninstKey = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\SIF Studio_is1"
if (Test-Path $uninstKey) {
    $ip = Get-ItemProperty -Path $uninstKey
    Add "NSIS Name"             $ip.DisplayName
    Add "NSIS Version"          $ip.DisplayVersion
    Add "NSIS InstallLocation"  $ip.InstallLocation
    Add "NSIS Publisher"        $ip.Publisher
    Add "NSIS InstallDate"      $ip.InstallDate
}

$msi32 = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"
if (Test-Path $msi32) {
    $msiEntry = Get-ChildItem $msi32 | Get-ItemProperty | Where-Object { $_.DisplayName -like "*SIF Studio*" } | Select-Object -First 1
    if ($msiEntry) {
        Add "MSI Name"             $msiEntry.DisplayName
        Add "MSI InstallLocation"  $msiEntry.InstallLocation
    }
}

Section "Binary checks"
$exeCandidates = @(
    "$env:LOCALAPPDATA\Programs\SIF Studio\sif-studio.exe",
    "$env:LOCALAPPDATA\Programs\SIF Studio\SIF Studio.exe"
)
foreach ($exe in $exeCandidates) {
    if (Test-Path $exe) {
        $size = (Get-Item $exe).Length
        $hash = (Get-FileHash $exe -Algorithm SHA256).Hash
        Add "exe path"   $exe
        Add "exe size"   "$size bytes"
        Add "exe SHA256" $hash
    }
}

Section "AppData data dir"
$dataDir = Join-Path $env:APPDATA "sif-studio"
if (Test-Path $dataDir) {
    Add "data dir" $dataDir
    Get-ChildItem $dataDir -Force | ForEach-Object {
        Add ("  " + $_.Name) ("{0} bytes  {1}" -f $_.Length, $_.LastWriteTime)
    }
} else {
    Add "data dir" "missing  <-- app never started (no DB created yet)"
}

Section "Process scan"
$proc = Get-Process | Where-Object { $_.ProcessName -like "*sif*" } | Select-Object ProcessName, Id, MainWindowTitle, StartTime
if ($proc) {
    $proc | Format-Table -AutoSize | Out-String -Stream | ForEach-Object { Add ("  " + $_).TrimEnd() "" }
} else {
    Add "process scan" "no sif-studio process running (either never started or already exited)"
}

Section "Windows Event Log (last 2 hours, errors from Application Error / .NET Runtime / WebView2 / Tauri / Msiexec)"
try {
    $evt = Get-WinEvent -FilterHashtable @{
        LogName = 'Application'
        Level = 1,2,3
        StartTime = (Get-Date).AddHours(-2)
    } -MaxEvents 20 -ErrorAction SilentlyContinue | Where-Object {
        $_.ProviderName -match "Application Error|.NET Runtime|WebView2|Tauri|Msiexec"
    }
    if ($evt) {
        $evt | Select-Object -First 10 TimeCreated, ProviderName, Id, LevelDisplayName, Message | Format-List | Out-String -Stream | ForEach-Object {
            $truncated = if ($_.Length -gt 300) { $_.Substring(0, 300) + "..." } else { $_ }
            Add ("  " + $truncated) ""
        }
    } else {
        Add "event log" "no related errors in last 2 hours"
    }
} catch {
    Add "event log" "(query failed: $($_.Exception.Message))"
}

Section "Path sanity (Chinese username / spaces are common pitfalls)"
$cjk = ($env:USERNAME -match '[\u4e00-\u9fff]')
$hasSpace = ($env:USERPROFILE -match '\s')
Add "username has CJK chars" $cjk
Add "USERPROFILE has spaces"  $hasSpace

Section "Diagnostic summary"
$diagnoses = @()
if (-not $found) { $diagnoses += "WebView2 missing -> install: https://developer.microsoft.com/microsoft-edge/webview2/" }
if (-not (Test-Path $vcKey)) { $diagnoses += "VC++ 2015+ x64 missing -> install: https://aka.ms/vs/17/release/vc_redist.x64.exe" }
if (-not (Test-Path (Join-Path $env:APPDATA "sif-studio"))) { $diagnoses += "DB dir not created -> app never reached first run" }
$procCount = (Get-Process | Where-Object { $_.ProcessName -like "*sif*" }).Count
if ($procCount -eq 0) { $diagnoses += "no sif-studio process running -> check Event Log above for crash reason" }

if ($diagnoses.Count -gt 0) {
    Write-Host ""
    Write-Host "Top issues to fix:" -ForegroundColor Yellow
    foreach ($d in $diagnoses) { Add "  ACTION" $d }
} else {
    Add "STATUS" "basic environment OK - paste this full output back to developer"
}

# Save UTF-8 report file (UTF-8 OK here; the PROBLEM was the .ps1 SCRIPT string literals)
$reportPath = Join-Path $env:TEMP ("sif-studio-diagnose-" + (Get-Date -Format 'yyyyMMdd-HHmmss') + ".txt")
$out | Out-File -FilePath $reportPath -Encoding UTF8
Write-Host ""
Write-Host "Report saved to:" -ForegroundColor Green
Write-Host ("  " + $reportPath)
Write-Host ""
Write-Host "Paste the entire contents of that .txt file back into the chat." -ForegroundColor Green
