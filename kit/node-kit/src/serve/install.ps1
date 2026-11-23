# Ahjo's apps for this Windows machine, from the Ahjo node at {{NODE}}
# (Ahjo {{VERSION}}). In PowerShell:
#
#   irm {{NODE}}/install.ps1 | iex
#
# It puts Ahjo Swarm, Ahjo Brain and Ahjo Desk in %LOCALAPPDATA%\Ahjo, adds
# them to the Start menu, points them at that node and opens Ahjo Swarm. No
# administrator rights. Run it again to repair; the apps update themselves
# from the node (Update, at the top of each window).
#
# For a trial run elsewhere, AHJO_INSTALL_DIR (the apps), AHJO_CONFIG_DIR
# (their settings, %APPDATA%\Ahjo)
# and AHJO_START_MENU (the shortcuts) move what it writes, and then it
# opens nothing.
#
# This file is ASCII on purpose: Windows PowerShell reads a script without a
# byte-order mark in the ANSI code page.

& {
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$node = '{{NODE}}'
$desk = '{{DESK}}'
$trial = [bool]$env:AHJO_INSTALL_DIR
$dir = if ($env:AHJO_INSTALL_DIR) { $env:AHJO_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Ahjo' }
$config = if ($env:AHJO_CONFIG_DIR) { $env:AHJO_CONFIG_DIR } else { Join-Path $env:APPDATA 'Ahjo' }
$menu = if ($env:AHJO_START_MENU) { $env:AHJO_START_MENU } else { [Environment]::GetFolderPath('Programs') }

# UTF-8 without a byte-order mark: the apps' JSON reader refuses one.
function Write-Text($path, $text) {
  [IO.File]::WriteAllText($path, $text, (New-Object Text.UTF8Encoding $false))
}

# The node's own words for a failed request, else the request's.
function Why($failure) {
  try { ($failure.ErrorDetails.Message | ConvertFrom-Json).error.message }
  catch { $failure.Exception.Message }
}

try { $manifest = Invoke-RestMethod -UseBasicParsing "$node/apps/windows" }
catch { throw "Nothing was installed: $(Why $_)" }

New-Item -ItemType Directory -Force -Path $dir, $config, $menu | Out-Null
$part = Join-Path $dir 'download'
if (Test-Path -LiteralPath $part) { Remove-Item -LiteralPath $part -Recurse -Force }
New-Item -ItemType Directory -Path $part | Out-Null
$total = ($manifest.files | Measure-Object -Property size -Sum).Sum
Write-Host ("Ahjo {0} from {1}, {2:N0} MB" -f $manifest.version, $node, ($total / 1MB))
try {
  foreach ($f in $manifest.files) {
    $to = Join-Path $part $f.name
    Write-Host "  $($f.name)"
    try { Invoke-WebRequest -UseBasicParsing -Uri "$node/apps/windows/$($f.name)" -OutFile $to }
    catch { throw "Nothing was installed: $($f.name) did not download: $(Why $_)" }
    $sum = (Get-FileHash -Algorithm SHA256 -LiteralPath $to).Hash.ToLowerInvariant()
    if ($sum -ne $f.sha256) {
      throw "Nothing was installed: $($f.name) arrived damaged (its SHA-256 is not the node's). Run the line again."
    }
  }
} catch {
  Remove-Item -LiteralPath $part -Recurse -Force -ErrorAction SilentlyContinue
  throw
}

# Every file is here and whole: put each in place. A running app keeps
# running from its old file under another name, removed on a later run.
$stamp = Get-Date -Format 'yyyyMMddHHmmss'
foreach ($f in $manifest.files) {
  $at = Join-Path $dir $f.name
  if (Test-Path -LiteralPath $at) { Move-Item -LiteralPath $at -Destination "$at.$stamp.old" -Force }
  Move-Item -LiteralPath (Join-Path $part $f.name) -Destination $at
}
Remove-Item -LiteralPath $part -Recurse -Force
Get-ChildItem -LiteralPath $dir -Filter '*.old' | Remove-Item -Force -ErrorAction SilentlyContinue

# Where the apps look for their updates.
Write-Text (Join-Path $dir 'install.json') (ConvertTo-Json @{ from = $node })

# Ahjo Swarm's node, which Brain's Send and Desk's notes to sessions use too.
$app = Join-Path $config 'app.json'
if (Test-Path -LiteralPath $app) {
  try { $settings = [IO.File]::ReadAllText($app) | ConvertFrom-Json }
  catch { throw "The apps are installed, but $app does not read as JSON; fix or remove it and run the line again." }
} else { $settings = New-Object PSObject }
if ($settings.node -ne $node) {
  $settings | Add-Member -Force -NotePropertyName node -NotePropertyValue $node
  Write-Text $app ($settings | ConvertTo-Json -Depth 32)
}

# Ahjo Brain reads the brain through the same node. It is kept in Brain's
# settings (brain.json, beside app.json), not on its shortcut, so a taskbar
# pin of the bare exe starts the same app as the Start menu. Brain used to
# keep them in its own folder: they are carried over here, and the old file
# removed once the new one is written.
$brainJson = Join-Path $config 'brain.json'
$brainOld = Join-Path (Join-Path $(if ($env:AHJO_CONFIG_DIR) { $config } else { $env:APPDATA }) 'ahjo-brain') 'settings.json'
$brainFrom = $null
if (Test-Path -LiteralPath $brainJson) { $brainFrom = $brainJson }
elseif (Test-Path -LiteralPath $brainOld) { $brainFrom = $brainOld }
if ($brainFrom) {
  try { $brain = [IO.File]::ReadAllText($brainFrom) | ConvertFrom-Json }
  catch { throw "The apps are installed, but $brainFrom does not read as JSON; fix or remove it and run the line again." }
} else { $brain = New-Object PSObject }
$brainWrite = ($brainFrom -ne $brainJson) -or ($brain.from -ne $node)
if ($brain.from -ne $node) {
  $brain | Add-Member -Force -NotePropertyName from -NotePropertyValue $node
}
if ($brainWrite) { Write-Text $brainJson ($brain | ConvertTo-Json -Depth 32) }
if (Test-Path -LiteralPath $brainOld) {
  Remove-Item -LiteralPath $brainOld -Force
  Remove-Item -LiteralPath (Split-Path $brainOld) -ErrorAction SilentlyContinue
}

# Ahjo Desk's first desk, unless it already has its own.
$deskJson = Join-Path $config 'desk.json'
if ($desk -and -not (Test-Path -LiteralPath $deskJson)) {
  Write-Text $deskJson (ConvertTo-Json -Depth 4 @{ desks = @(@{ name = ''; url = $desk }) })
}

$shell = New-Object -ComObject WScript.Shell
# Each shortcut is the bare exe: everything an app needs is in its settings,
# so pinning a running window to the taskbar pins the same app.
$links = @(
  @{ name = 'Ahjo Swarm'; exe = 'ahjo-swarm.exe' },
  @{ name = 'Ahjo Brain'; exe = 'ahjo-brain.exe' },
  @{ name = 'Ahjo Desk'; exe = 'ahjo-desk.exe' }
)
foreach ($l in $links) {
  $exe = Join-Path $dir $l.exe
  if (-not (Test-Path -LiteralPath $exe)) { continue }
  $link = $shell.CreateShortcut((Join-Path $menu "$($l.name).lnk"))
  $link.TargetPath = $exe
  $link.Arguments = ''
  $link.WorkingDirectory = $dir
  $link.Description = $l.name
  $link.Save()
}

Write-Host "Installed in $dir. Ahjo Swarm, Ahjo Brain and Ahjo Desk are in the Start menu."
Write-Host "Each offers Update when $node runs a newer build."
if (-not $trial) { Start-Process -FilePath (Join-Path $dir 'ahjo-swarm.exe') -WorkingDirectory $dir }
}
