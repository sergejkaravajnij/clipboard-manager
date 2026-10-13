$ErrorActionPreference = 'Stop'
$taskRoot = $PSScriptRoot
$cargoExe = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
Push-Location -LiteralPath $taskRoot
try {
    & $cargoExe build --release
    if ($LASTEXITCODE -ne 0) { throw 'Build failed.' }
    $appExe = Join-Path $taskRoot 'clipboard-manager.exe'
    Copy-Item -LiteralPath (Join-Path $taskRoot 'target\release\clipboard-manager.exe') -Destination $appExe -Force
    $runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
    New-Item -Path $runKey -Force | Out-Null
    New-ItemProperty -Path $runKey -Name 'ClipboardManager' -Value ('"' + $appExe + '"') -PropertyType String -Force | Out-Null
    Start-Process -FilePath $appExe -WindowStyle Hidden
    Write-Output "Installed and started: $appExe"
} finally { Pop-Location }
