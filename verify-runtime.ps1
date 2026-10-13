$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ClipboardManagerTestUi {
    [DllImport("user32.dll", EntryPoint="FindWindowW", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string className, IntPtr title);
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr window, int id);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, UIntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr window, uint message, UIntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern bool SetWindowText(IntPtr window, string text);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr window, System.Text.StringBuilder text, int count);
}
'@

$fixture = (Get-Content -LiteralPath (Join-Path $PSScriptRoot 'artifacts\runtime-fixture-path.txt') -Raw).Trim()
$exe = Join-Path $PSScriptRoot 'target\release\clipboard-manager.exe'
$report = [ordered]@{ exeBytes = (Get-Item -LiteralPath $exe).Length; fixtureBytes = (Get-Item -LiteralPath (Join-Path $fixture 'history.sqlite')).Length }

for ($iteration = 0; $iteration -lt 2; $iteration++) {
    $process = Start-Process -FilePath $exe -ArgumentList ('"--data-dir=' + $fixture + '"') -WindowStyle Hidden -PassThru
    try {
        $window = [IntPtr]::Zero
        for ($attempt = 0; $attempt -lt 50; $attempt++) {
            Start-Sleep -Milliseconds 100
            $window = [ClipboardManagerTestUi]::FindWindow('ClipboardManagerWindow', [IntPtr]::Zero)
            if ($window -ne [IntPtr]::Zero) { break }
            if ($process.HasExited) { throw 'Release process exited unexpectedly.' }
        }
        if ($window -eq [IntPtr]::Zero) { throw 'Release window was not created.' }
        [ClipboardManagerTestUi]::PostMessage($window, 0x8002, [UIntPtr]1, [IntPtr]::Zero) | Out-Null
        Start-Sleep -Milliseconds 200
        $search = [ClipboardManagerTestUi]::GetDlgItem($window, 101)
        foreach ($character in 'needle-9999'.ToCharArray()) {
            [ClipboardManagerTestUi]::SendMessage($search, 0x102, [UIntPtr]([int]$character), [IntPtr]::Zero) | Out-Null
        }
        $list = [ClipboardManagerTestUi]::GetDlgItem($window, 102)
        for ($attempt = 0; $attempt -lt 30; $attempt++) {
            Start-Sleep -Milliseconds 100
            $count = [ClipboardManagerTestUi]::SendMessage($list, 0x18B, [UIntPtr]::Zero, [IntPtr]::Zero).ToInt64()
            if ($count -eq 1) { break }
        }
        if ($count -ne 1) {
            $text = [System.Text.StringBuilder]::new(256)
            [ClipboardManagerTestUi]::GetWindowText($search, $text, 256) | Out-Null
            throw "Persisted filter returned $count rows instead of 1; search=$text."
        }
        [ClipboardManagerTestUi]::PostMessage($window, 0x8002, [UIntPtr]9, [IntPtr]::Zero) | Out-Null
        Start-Sleep -Milliseconds 300
        $process.Refresh()
        $cpuBefore = $process.TotalProcessorTime.TotalMilliseconds
        Start-Sleep -Seconds 2
        $process.Refresh()
        $report["run${iteration}WorkingSetBytes"] = $process.WorkingSet64
        $report["run${iteration}PrivateBytes"] = $process.PrivateMemorySize64
        $report["run${iteration}IdleCpuMillisecondsOver2s"] = $process.TotalProcessorTime.TotalMilliseconds - $cpuBefore
        $report["run${iteration}PersistedSearchRows"] = $count
    } finally {
        if ($window -ne [IntPtr]::Zero) {
            [ClipboardManagerTestUi]::PostMessage($window, 0x8002, [UIntPtr]12, [IntPtr]::Zero) | Out-Null
            if (-not $process.WaitForExit(5000)) { throw 'Application failed to exit from its tray command.' }
        }
    }
}
$report | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'artifacts\runtime-report.json') -Encoding utf8
$report | ConvertTo-Json
