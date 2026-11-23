# The Windows half of try-windows: runs in the operator's desktop session
# (a one-off scheduled task with /IT) and acts only on the processes it
# started, by the ids it wrote. One step per run, named in the run folder's
# job.txt; done.txt (or failed.txt) says when it is over.
#
#   start <app> <exe> [args...]  start the app with the run's scratch settings,
#                                its window placed off every screen
#   shot <app> <png>             capture the app's window, wherever it is
#   stop                         stop every app this run started
param([string]$Run)
$ErrorActionPreference = "Stop"
$job = Get-Content -LiteralPath (Join-Path $Run "job.txt")
$words = $job -split "`t"
try {
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Win {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  public struct RECT { public int L, T, R, B; }
}
"@
    [Win]::SetProcessDPIAware() | Out-Null

    # The window of process $id, once it shows. $script:polled is how long
    # the last look took with the wait before it: the longest a window can
    # have shown before it was seen.
    function Window-Of([int]$id) {
        $p = Get-Process -Id $id
        $clock = [Diagnostics.Stopwatch]::StartNew()
        while ($clock.Elapsed.TotalSeconds -lt 60) {
            $lap = $clock.Elapsed.TotalMilliseconds
            $p.Refresh()
            if ($p.HasExited) { throw "process $id exited before its window opened" }
            if ($p.MainWindowHandle -ne 0) {
                $script:polled = $clock.Elapsed.TotalMilliseconds - $lap + 20
                return $p.MainWindowHandle
            }
            Start-Sleep -Milliseconds 20
        }
        throw "process $id opened no window in 60 s"
    }

    switch ($words[0]) {
        "start" {
            $app, $exe = $words[1], $words[2]
            $rest = @($words | Select-Object -Skip 3)
            # Everything the app keeps goes to the run: its settings, log,
            # web view data and caches.
            foreach ($line in Get-Content -LiteralPath (Join-Path $Run "env.txt")) {
                $k, $v = $line -split "=", 2
                [Environment]::SetEnvironmentVariable($k, $v, "Process")
            }
            $right = [System.Windows.Forms.SystemInformation]::VirtualScreen.Right
            $p = if ($rest.Count) { Start-Process -FilePath $exe -ArgumentList $rest -PassThru } else { Start-Process -FilePath $exe -PassThru }
            Set-Content -LiteralPath (Join-Path $Run "$app.pid") -Value $p.Id, $p.Path
            $h = Window-Of $p.Id
            # The app opens on a screen (gpui puts a window it is placed off
            # every screen back on the main one), so move it right of the
            # rightmost screen at once, without activating it. Windows keeps
            # drawing it there, and PrintWindow reads it.
            $move = [Diagnostics.Stopwatch]::StartNew()
            [Win]::SetWindowPos($h, [IntPtr]::Zero, $right + 400, 0, 0, 0, 0x0001 -bor 0x0004 -bor 0x0010) | Out-Null
            $shown = [int]($script:polled + $move.Elapsed.TotalMilliseconds)
            $fg = [Win]::GetForegroundWindow(); $fgpid = 0
            [Win]::GetWindowThreadProcessId($fg, [ref]$fgpid) | Out-Null
            $r = New-Object Win+RECT; [Win]::GetWindowRect($h, [ref]$r) | Out-Null
            "pid $($p.Id) window $($r.L),$($r.T),$($r.R),$($r.B) screens-right $right on-screen-at-most ${shown}ms took-focus $($fgpid -eq $p.Id)" |
                Set-Content -LiteralPath (Join-Path $Run "$app.start.txt")
        }
        "shot" {
            $app, $png = $words[1], $words[2]
            $id = [int]((Get-Content -LiteralPath (Join-Path $Run "$app.pid"))[0])
            $h = Window-Of $id
            $r = New-Object Win+RECT; [Win]::GetWindowRect($h, [ref]$r) | Out-Null
            $w, $ht = ($r.R - $r.L), ($r.B - $r.T)
            $b = New-Object System.Drawing.Bitmap $w, $ht
            $g = [System.Drawing.Graphics]::FromImage($b)
            $dc = $g.GetHdc()
            # 2: PW_RENDERFULLCONTENT, which draws what DirectComposition
            # shows, off screen or covered as well.
            $ok = [Win]::PrintWindow($h, $dc, 2)
            $g.ReleaseHdc($dc); $g.Dispose()
            if (-not $ok) { throw "PrintWindow refused $app's window" }
            $b.Save($png, [System.Drawing.Imaging.ImageFormat]::Png); $b.Dispose()
        }
        "stop" {
            foreach ($f in Get-ChildItem -LiteralPath $Run -Filter "*.pid") {
                $id, $path = Get-Content -LiteralPath $f.FullName
                # Only the process this run started: the same id and program.
                $p = Get-Process -Id ([int]$id) -ErrorAction SilentlyContinue
                if ($p -and $p.Path -eq $path) {
                    Stop-Process -Id $p.Id -Force
                    Wait-Process -Id $p.Id -Timeout 10 -ErrorAction SilentlyContinue
                }
                Remove-Item -LiteralPath $f.FullName
            }
        }
        default { throw "unknown step '$($words[0])'" }
    }
    Set-Content -LiteralPath (Join-Path $Run "done.txt") -Value "ok"
} catch {
    Set-Content -LiteralPath (Join-Path $Run "failed.txt") -Value "$($words[0]): $_"
}
