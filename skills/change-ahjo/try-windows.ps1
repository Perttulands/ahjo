# The Windows half of try-windows: runs in the operator's desktop session
# (a one-off scheduled task with /IT) and acts only on the processes it
# started, by the ids it wrote. One step per run, named in the run folder's
# job.txt; done.txt (or failed.txt) says when it is over.
#
#   start <app> <exe> [args...]  start the app with the run's scratch settings,
#                                its window placed off every screen
#   shot <app> <png>             capture the app's window, wherever it is
#   click <app> <x> <y>          a left click at the screenshot's pixel x, y
#   type <app> <text>            the text, as typed characters
#   key <app> <key>              one key without modifiers (enter, escape...)
#   eval <app> <script>          the value of a script in the app's web page,
#                                as JSON in result.txt
#   stop                         stop every app this run started
#
# Clicks and keys are posted to the app's own windows, so it need not be in
# front and nothing else on the desktop sees them. A click goes to the
# deepest child window under its pixel, such as a web view's, as a real one
# would. A posted key carries no Alt or Ctrl: Windows reads those from the
# keyboard itself, and goes to the window that has the keyboard, the app's
# or a child's.
#
# An app's web view answers eval through its DevTools port: start gives each
# app a free one (WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS), kept in
# <app>.devtools.
param([string]$Run)
$ErrorActionPreference = "Stop"
# Words cross between WSL and here as UTF-8 without a BOM, both ways:
# Windows PowerShell's own default is the system code page, which turns a
# page's or a step's other characters into "?".
function Write-Utf8([string]$name, [string]$text) {
    [IO.File]::WriteAllText((Join-Path $Run $name), $text, (New-Object Text.UTF8Encoding $false))
}
# The answer to one script in a page, through its DevTools socket.
function Invoke-Page($page, [string]$script) {
    $ws = New-Object System.Net.WebSockets.ClientWebSocket
    $none = [Threading.CancellationToken]::None
    $ws.ConnectAsync([Uri]$page.webSocketDebuggerUrl, $none).Wait() | Out-Null
    $ask = @{ id = 1; method = "Runtime.evaluate";
              params = @{ expression = $script; returnByValue = $true; awaitPromise = $true } } | ConvertTo-Json -Compress -Depth 5
    $bytes = [Text.Encoding]::UTF8.GetBytes($ask)
    $ws.SendAsync((New-Object ArraySegment[byte] (,$bytes)), "Text", $true, $none).Wait() | Out-Null
    # Events may come first: read messages until the answer to id 1.
    $answer = $null
    while (-not $answer) {
        $text = New-Object System.Text.StringBuilder
        do {
            $buffer = New-Object byte[] 65536
            $got = $ws.ReceiveAsync((New-Object ArraySegment[byte] (,$buffer)), $none)
            if (-not $got.Wait(20000)) { throw "the page did not answer the script in 20 s" }
            $text.Append([Text.Encoding]::UTF8.GetString($buffer, 0, $got.Result.Count)) | Out-Null
        } until ($got.Result.EndOfMessage)
        $message = $text.ToString() | ConvertFrom-Json
        if ($message.id -eq 1) { $answer = $message }
    }
    $ws.Dispose()
    $answer
}
$job = [IO.File]::ReadAllText((Join-Path $Run "job.txt"), (New-Object Text.UTF8Encoding $false))
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
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool ScreenToClient(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern IntPtr ChildWindowFromPointEx(IntPtr h, POINT p, uint flags);
  [DllImport("user32.dll")] public static extern bool GetGUIThreadInfo(uint thread, ref GUITHREADINFO info);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, System.Text.StringBuilder name, int max);
  public struct RECT { public int L, T, R, B; }
  public struct POINT { public int X, Y; }
  [StructLayout(LayoutKind.Sequential)] public struct GUITHREADINFO {
    public int cbSize; public uint flags; public IntPtr active, focus, capture, menuOwner, moveSize, caret; public RECT caretRect;
  }
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

    # The deepest visible child of window $h under screen point $x, $y, or
    # $h itself, as the point in that window's client area.
    function Target-At([IntPtr]$h, [int]$x, [int]$y) {
        while ($true) {
            $p = New-Object Win+POINT; $p.X = $x; $p.Y = $y
            [Win]::ScreenToClient($h, [ref]$p) | Out-Null
            # 1 | 4: skip invisible and transparent children.
            $c = [Win]::ChildWindowFromPointEx($h, $p, 0x0001 -bor 0x0004)
            if ($c -eq [IntPtr]::Zero -or $c -eq $h) { return $h, $p }
            $h = $c
        }
    }

    function Class-Of([IntPtr]$h) {
        $name = New-Object System.Text.StringBuilder 256
        [Win]::GetClassName($h, $name, 256) | Out-Null
        $name.ToString()
    }

    # The window of the app's that has the keyboard (a web view's child when
    # it does), else its own.
    function Focus-Of([IntPtr]$h) {
        $id = 0
        $thread = [Win]::GetWindowThreadProcessId($h, [ref]$id)
        $info = New-Object Win+GUITHREADINFO
        $info.cbSize = [Runtime.InteropServices.Marshal]::SizeOf($info)
        if ([Win]::GetGUIThreadInfo($thread, [ref]$info) -and $info.focus -ne [IntPtr]::Zero) { return $info.focus }
        $h
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
            # A free port for the app's web views' DevTools, for eval.
            $l = New-Object System.Net.Sockets.TcpListener ([Net.IPAddress]::Loopback), 0
            $l.Start(); $devtools = $l.LocalEndpoint.Port; $l.Stop()
            [Environment]::SetEnvironmentVariable("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--remote-debugging-port=$devtools", "Process")
            Set-Content -LiteralPath (Join-Path $Run "$app.devtools") -Value $devtools
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
        "click" {
            $app, $x, $y = $words[1], [int]$words[2], [int]$words[3]
            $h = Window-Of ([int]((Get-Content -LiteralPath (Join-Path $Run "$app.pid"))[0]))
            # A screenshot is the whole window: its pixel is the window's
            # corner plus x, y on the screen, made the client area's of the
            # deepest child there.
            $r = New-Object Win+RECT; [Win]::GetWindowRect($h, [ref]$r) | Out-Null
            $h, $p = Target-At $h ($r.L + $x) ($r.T + $y)
            Write-Utf8 "result.txt" (Class-Of $h)
            $at = [IntPtr](($p.Y -shl 16) -bor ($p.X -band 0xffff))
            [Win]::PostMessage($h, 0x0200, [IntPtr]::Zero, $at) | Out-Null  # WM_MOUSEMOVE
            Start-Sleep -Milliseconds 150
            [Win]::PostMessage($h, 0x0201, [IntPtr]1, $at) | Out-Null       # WM_LBUTTONDOWN
            Start-Sleep -Milliseconds 80
            [Win]::PostMessage($h, 0x0202, [IntPtr]::Zero, $at) | Out-Null  # WM_LBUTTONUP
            Start-Sleep -Milliseconds 700
        }
        "type" {
            $app, $text = $words[1], ($words[2..($words.Count - 1)] -join " ")
            $h = Focus-Of (Window-Of ([int]((Get-Content -LiteralPath (Join-Path $Run "$app.pid"))[0])))
            Write-Utf8 "result.txt" (Class-Of $h)
            foreach ($c in $text.ToCharArray()) {
                [Win]::PostMessage($h, 0x0102, [IntPtr][int]$c, [IntPtr]1) | Out-Null  # WM_CHAR
                Start-Sleep -Milliseconds 20
            }
            Start-Sleep -Milliseconds 300
        }
        "key" {
            $app, $name = $words[1], $words[2]
            $vk = @{ enter = 0x0D; escape = 0x1B; tab = 0x09; backspace = 0x08;
                     left = 0x25; up = 0x26; right = 0x27; down = 0x28 }[$name]
            if (-not $vk) { throw "no key '$name'" }
            $h = Focus-Of (Window-Of ([int]((Get-Content -LiteralPath (Join-Path $Run "$app.pid"))[0])))
            Write-Utf8 "result.txt" (Class-Of $h)
            [Win]::PostMessage($h, 0x0100, [IntPtr]$vk, [IntPtr]1) | Out-Null           # WM_KEYDOWN
            Start-Sleep -Milliseconds 50
            [Win]::PostMessage($h, 0x0101, [IntPtr]$vk, [IntPtr]0xC0000001) | Out-Null  # WM_KEYUP
            Start-Sleep -Milliseconds 500
        }
        "eval" {
            $app, $script = $words[1], ($words[2..($words.Count - 1)] -join " ")
            $port = [int](Get-Content -LiteralPath (Join-Path $Run "$app.devtools"))
            # The app's pages, once its web views have loaded one; of
            # several, the one shown (a hidden view's page is hidden).
            $clock = [Diagnostics.Stopwatch]::StartNew()
            $pages = @()
            while ($pages.Count -eq 0 -and $clock.Elapsed.TotalSeconds -lt 20) {
                try {
                    # Windows PowerShell hands a JSON array on as one object:
                    # assigned first, it is taken apart item by item.
                    $list = Invoke-RestMethod -Uri "http://127.0.0.1:$port/json/list" -TimeoutSec 2
                    $pages = @($list | ForEach-Object { $_ } |
                        Where-Object { $_.type -eq "page" -and $_.url -notlike "about:*" })
                } catch { }
                if ($pages.Count -eq 0) { Start-Sleep -Milliseconds 250 }
            }
            if ($pages.Count -eq 0) { throw "$app has no web page loaded (DevTools port $port)" }
            $page = $pages[0]
            if ($pages.Count -gt 1) {
                foreach ($p in $pages) {
                    $seen = (Invoke-Page $p "document.visibilityState").result.result.value
                    if ($seen -eq "visible") { $page = $p; break }
                }
            }
            $answer = Invoke-Page $page $script
            if ($answer.result.exceptionDetails) {
                throw "the script threw: $($answer.result.exceptionDetails.exception.description)"
            }
            Write-Utf8 "result.txt" (ConvertTo-Json -InputObject $answer.result.result.value -Compress -Depth 10)
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
            # A killed app leaves its web view's processes holding the run's
            # folder for a while: only those whose data is in this run.
            $mine = { @(Get-CimInstance Win32_Process -Filter "Name = 'msedgewebview2.exe'" |
                Where-Object { $_.CommandLine -and $_.CommandLine.IndexOf($Run, [StringComparison]::OrdinalIgnoreCase) -ge 0 }) }
            foreach ($w in & $mine) { Stop-Process -Id $w.ProcessId -Force -ErrorAction SilentlyContinue }
            $clock = [Diagnostics.Stopwatch]::StartNew()
            while ((& $mine).Count -gt 0 -and $clock.Elapsed.TotalSeconds -lt 10) { Start-Sleep -Milliseconds 250 }
        }
        default { throw "unknown step '$($words[0])'" }
    }
    Set-Content -LiteralPath (Join-Path $Run "done.txt") -Value "ok"
} catch {
    Write-Utf8 "failed.txt" "$($words[0]): $_"
}
