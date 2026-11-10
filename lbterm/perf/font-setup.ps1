#Requires -Version 7
# Covers: wezterm-font/src/lib.rs, wezterm-font/src/lbterm.rs, wezterm-font/src/locator/gdi.rs, wezterm-font/src/db.rs, wezterm-font/src/parser.rs, wezterm-gui/src/termwindow/mod.rs, wezterm-gui/src/termwindow/resize.rs, wezterm-gui/src/termwindow/render/paint.rs, wezterm-gui/src/termwindow/render/mod.rs, wezterm-gui/src/renderstate.rs, wezterm-gui/src/glyphcache.rs, wezterm-gui/src/counters.rs, wezterm-gui/src/termwindow/webgpu.rs, wezterm-gui/src/termwindow/gpu_warm.rs, wezterm-gui/src/main.rs, config/src/lbterm.rs, lbterm-extras/src/start.rs
<#
What font setup costs (lbterm/PERF.md, "What font setup costs"), held by counts: a test window
drawn as Lachlan's (New-TestConfig: the bundled JetBrains Mono, and Symbols Nerd Font Mono beside
it in the window frame's font), each step read from the control pipe's metrics `fonts` (wezterm-font's
lbterm.rs, the GUI's counters.rs), reset before it. Counts, not times, so it holds locked or not
and beside other sessions' builds; the budget runner's reload, font-zoom, new-window,
fallback-line and start scenarios hold the times.

1. The start: the bundled fonts parsed once, no system font file read but the window's caption
   font's (GDI's lookup of a family Windows doesn't have turned down by the name of the face it
   put in its place, not by reading that face's whole file), the glyph atlas never cleared while
   it fills (it grows), the window's start said step by step in metrics' `start`.
2. Twenty lines and a tab title with characters none of the window's fonts has: found by
   searching, some of it answered from what another font found (`fallback_shared`), and never
   more searches than the fonts that met them.
3. A reload of the same config (ReloadConfiguration): the fonts kept, so nothing made, looked up,
   read, searched for, no atlas made and no reshape.
4. A zoom a size bigger and back: fonts made at the new size, but nothing searched for (what was
   found is kept across sizes) and no system file read.
5. The config's font size changed (a window override, which runs config_was_reloaded): fonts
   made again, still nothing searched for.
6. A new window (wezterm.mux.spawn_window): the bundled fonts not parsed again, nothing
   searched for (the windows share what was found), no file read (the caption font kept).

  pwsh -File lbterm\perf\font-setup.ps1 [-Bin <build>]
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'font-setup' -Bin $Bin

# The lines and the title: symbols Claude Code prints, Han and Hangul, as code points
$Symbols = -join (@(0x23FA, 0x20, 0x23BF, 0x20, 0x273B, 0x20, 0x26A0, 0x20, 0x6F22, 0x5B57, 0x20, 0xD55C, 0xAD6D, 0x20, 0x2605) | ForEach-Object { [char]$_ })

$steps = [Collections.Generic.List[object]]::new()
function Get-Fonts($m) {
  $f = $m.fonts
  if (-not $f) { throw "this build's metrics have no fonts: it's from before font setup was counted" }
  $o = [ordered]@{}
  foreach ($p in $f.PSObject.Properties) { $o[$p.Name] = if ($p.Value -is [pscustomobject]) { [int]$p.Value.count } else { $p.Value } }
  [pscustomobject]$o
}
# What one step did: metrics reset, the step, the window quiet again, metrics
function Measure-Step([string]$Name, $Window, [scriptblock]$Do) {
  Get-Metrics -Window $Window -Reset | Out-Null
  & $Do
  Get-QuietFrame -QuietMs 700 -TimeoutSec 20 -Window $Window | Out-Null
  $f = Get-Fonts (Get-Metrics -Window $Window)
  $steps.Add([pscustomobject]@{ Step = $Name; Fonts = $f })
  Write-Host ("{0}: fonts {1}, lookups {2}, files read {3}, substitutes {4}, built-in {5}, searches {6}, shared {7}, shapers {8}, kept {9}, atlases {10} (cleared {11}, grown {12}, side {13}), reshapes {14}" -f $Name, $f.fonts, $f.lookups, $f.files_read, $f.substitutes, $f.built_in, $f.fallback_searches, $f.fallback_shared, $f.shapers, $f.kept, $f.atlas_made, $f.atlas_cleared, $f.atlas_grown, $f.atlas_side, $f.reshapes)
  $f
}
function Assert-Count([string]$Step, [string]$What, $Value, [string]$Op, [int]$Bound) {
  $ok = switch ($Op) { 'eq' { $Value -eq $Bound } 'le' { $Value -le $Bound } 'ge' { $Value -ge $Bound } }
  if (-not $ok) { Add-Failure "$($Step): $What was $Value, where it should be $(@{ eq = ''; le = 'at most '; ge = 'at least ' }[$Op])$Bound" }
}

Invoke-Test {
  $x0, $y0 = (Get-OffscreenPosition -Screens @([System.Windows.Forms.Screen]::AllScreens | ForEach-Object Bounds) -Desktop ([System.Windows.Forms.SystemInformation]::VirtualScreen)) -split ','
  $cfg = Join-Path $T.Out 'config\font-setup.lua'
  New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
  New-TestConfig -Path $cfg -Lua @"
config.initial_cols = 110
config.initial_rows = 30
config.window_close_confirmation = 'NeverPrompt'
local pwsh = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' }
config.default_prog = pwsh
local act = wezterm.action
config.keys = config.keys or {}
for _, k in ipairs({
  { key = 'F5', mods = 'NONE', action = act.ReloadConfiguration },
  { key = 'F6', mods = 'NONE', action = act.CloseCurrentTab { confirm = false } },
  { key = 'F9', mods = 'NONE', action = act.IncreaseFontSize },
  { key = 'F10', mods = 'NONE', action = act.DecreaseFontSize },
  { key = 'F11', mods = 'NONE', action = wezterm.action_callback(function(window, pane)
      wezterm.mux.spawn_window { args = pwsh, position = { x = $x0 + 60, y = $y0 + 40, origin = 'ScreenCoordinateSystem' } }
    end) },
  { key = 'F12', mods = 'NONE', action = wezterm.action_callback(function(window, pane)
      window:set_config_overrides { font_size = 12.5 }
    end) },
}) do table.insert(config.keys, k) end
"@
  $w = Open-TestWindow -ConfigFile $cfg -Program (New-PwshProgram) -TimeoutSec 30
  Wait-State { param($s) @($s.panes).Count -ge 1 } -What 'its pane' -TimeoutSec 30 | Out-Null
  Get-QuietFrame -QuietMs 700 -TimeoutSec 20 -Window $w | Out-Null
  Send-Focus $w

  # 1. The start: everything since the process began
  $m = Get-Metrics -Window $w
  $f = Get-Fonts $m
  $steps.Add([pscustomobject]@{ Step = 'start'; Fonts = $f })
  Write-Host ("start: fonts {0}, lookups {1}, files read {2}, substitutes {3}, built-in {4}, atlases {5} (cleared {6}, grown {7}, side {8}); the window's start {9} ms, its GPU state {10} ms" -f $f.fonts, $f.lookups, $f.files_read, $f.substitutes, $f.built_in, $f.atlas_made, $f.atlas_cleared, $f.atlas_grown, $f.atlas_side, $m.start.window.ms, $m.start.webgpu.ms)
  Assert-Count 'start' 'the bundled fonts parsed' $f.built_in 'eq' 1
  # The window's caption font, once a process (wezterm-font's parse_log_font keeps it)
  Assert-Count 'start' 'system font files read' $f.files_read 'le' 1
  Assert-Count 'start' 'atlases cleared as the first frames filled them' $f.atlas_cleared 'eq' 0
  if (-not $m.start.window -or -not $m.start.webgpu) { Add-Failure "start: metrics' start has no window or webgpu steps: $($m.start | ConvertTo-Json -Compress)" }
  else { Write-Host "  window: $(($m.start.window.steps.PSObject.Properties | ForEach-Object { "$($_.Name) $($_.Value)" }) -join ', ')"; Write-Host "  webgpu: $(($m.start.webgpu.steps.PSObject.Properties | ForEach-Object { "$($_.Name) $($_.Value)" }) -join ', ')" }

  # 2. Characters no font of the window has, in the pane and its tab's title. In UTF-8 (the
  # console's code page makes them '?'), and a title of no use first: a program's first doesn't
  # reach the pane
  $pane = (Get-Panes)[0].pane_id
  $cmd = '[Console]::OutputEncoding = [Text.Encoding]::UTF8; 1..20 | % {{ ''{0} '' + $_ }}; [Console]::Write("`e]0;symbols`a"); Start-Sleep -Milliseconds 200; [Console]::Write("`e]0;{0}`a")' -f $Symbols
  $f = Measure-Step 'symbols' $w {
    Invoke-Cli send-text --pane-id $pane --no-paste "$cmd`r" -Check | Out-Null
    Wait-State { param($s) @($s.panes | Where-Object active)[0].title -eq $Symbols } -What 'the title with symbols' -TimeoutSec 20 | Out-Null
  }
  Assert-Count 'symbols' 'fallback searches' $f.fallback_searches 'ge' 1
  Assert-Count 'symbols' 'characters answered from what another font found' $f.fallback_shared 'ge' 1
  Assert-Count 'symbols' 'system font files read' $f.files_read 'eq' 0

  # 3. The same config again
  $f = Measure-Step 'reload' $w {
    Send-Key $w F5
    Wait-State { param($s) $s.strip.notice -match 'reload' } -What '"Config reloaded"' -TimeoutSec 10 | Out-Null
  }
  Assert-Count 'reload' 'reloads that kept the fonts' $f.kept 'eq' 1
  foreach ($k in 'fonts', 'lookups', 'files_read', 'fallback_searches', 'shapers', 'atlas_made', 'reshapes') { Assert-Count 'reload' $k $f.$k 'eq' 0 }

  # 4. A size bigger, and back; the readout's fade (DWM's) over between them
  foreach ($key in 'F9', 'F10') {
    $f = Measure-Step "zoom $key" $w {
      $was = (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).size.width
      Send-Key $w $key
      Wait-State { param($s) $s.size.width -ne $was } -What 'the window at its new size' -TimeoutSec 10 | Out-Null
      Start-Sleep -Milliseconds 1600
    }
    Assert-Count "zoom $key" 'fonts made at the new size' $f.fonts 'ge' 1
    Assert-Count "zoom $key" 'fallback searches' $f.fallback_searches 'eq' 0
    Assert-Count "zoom $key" 'system font files read' $f.files_read 'eq' 0
    Assert-Count "zoom $key" 'atlases cleared' $f.atlas_cleared 'eq' 0
  }

  # 5. The font size changed in the config (a window override)
  $f = Measure-Step 'font size' $w {
    $was = (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).size.width
    Send-Key $w F12
    Wait-Until { (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).size.width -ne $was } -TimeoutSec 10 | Out-Null
  }
  Assert-Count 'font size' 'reloads that kept the fonts' $f.kept 'eq' 0
  Assert-Count 'font size' 'fonts made' $f.fonts 'ge' 1
  Assert-Count 'font size' 'fallback searches' $f.fallback_searches 'eq' 0

  # 6. A new window, then closed
  $f = Measure-Step 'new window' $w {
    Send-Key $w F11
    if (-not (Wait-Until { @((Read-State).windows | Where-Object { [long]$_.frame -ge 1 }).Count -ge 2 } -TimeoutSec 20)) { throw 'the new window never painted' }
  }
  Assert-Count 'new window' 'the bundled fonts parsed again' $f.built_in 'eq' 0
  Assert-Count 'new window' 'fallback searches' $f.fallback_searches 'eq' 0
  Assert-Count 'new window' 'system font files read' $f.files_read 'eq' 0
  Assert-Count 'new window' 'atlases cleared' $f.atlas_cleared 'eq' 0
  foreach ($nw in @((Read-State).windows | Where-Object { [long]$_.hwnd -ne [long]$w.Handle })) { Send-Key ([IntPtr][long]$nw.hwnd) F6 }
  Wait-Until { @((Read-State).windows).Count -eq 1 } -TimeoutSec 15 | Out-Null

  [IO.File]::WriteAllText((Join-Path $T.Out 'counts.json'), ($steps | ConvertTo-Json -Depth 6))
} -Passed 'font setup: the bundled fonts parsed once and no system font file read, a fallback answered from what another font found, a reload of the same config keeping every font and the atlas, a zoom and a font size change searching for nothing, and a new window parsing and searching for nothing'
