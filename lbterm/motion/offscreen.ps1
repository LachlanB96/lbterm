# Where the motion scripts open their windows, so a test window never lands where Lachlan is
# typing: on the right-most screen (his far-right portrait monitor), a little in from its corner.
# Start the window with Start-Unfocused (start-unfocused.ps1, loaded here): no console, and
# LBTERM_NO_ACTIVATE, so it opens without taking the keyboard. Never `explorer.exe launch.cmd`:
# the .cmd's own console is a Windows Terminal window that comes up in front and swallows what
# Lachlan is typing.
#
#   . (Join-Path $PSScriptRoot 'offscreen.ps1')
#   $id = Start-Unfocused -Exe $gui -Arguments "start --position $TestWindowPosition ..."
#   Wait-Window -Id $id
. (Join-Path $PSScriptRoot 'start-unfocused.ps1')
Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class LbtermScreens { [DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); }
'@
# Real pixels, not the scaled ones a DPI-unaware process is shown
[void][LbtermScreens]::SetProcessDPIAware()

# Where a test window opens, as `start --position` takes it: 20 px in and 120 down from the top-left
# of the screen whose right edge is furthest right. `--position` counts from the top-left of the
# whole desktop ($Desktop, the virtual screen), not the main screen's corner. $Screens are the
# screens' bounds (anything with X, Y, Right).
function Get-OffscreenPosition {
  param([Parameter(Mandatory)][object[]]$Screens, [Parameter(Mandatory)]$Desktop)
  $screen = $Screens | Sort-Object { $_.Right } | Select-Object -Last 1
  '{0},{1}' -f ($screen.X - $Desktop.X + 20), ($screen.Y - $Desktop.Y + 120)
}
$TestWindowPosition = Get-OffscreenPosition -Screens @([System.Windows.Forms.Screen]::AllScreens | ForEach-Object Bounds) `
  -Desktop ([System.Windows.Forms.SystemInformation]::VirtualScreen)

# The lbterm.lua test windows look like: "config" in ~\.config\lbterm\dev.json, which names this
# PC's own paths so the repo needn't (Lachlan's is his real lbterm.lua, not the ~\.lbterm.lua
# that loads it and would put it on the reload watch list), else the first config LBTerm itself
# would load
function Get-LookalikeConfig {
  $dev = Join-Path $HOME '.config\lbterm\dev.json'
  if (Test-Path -LiteralPath $dev) {
    $config = (Get-Content -LiteralPath $dev -Raw | ConvertFrom-Json).config
    if ($config) { return $config }
  }
  foreach ($c in (Join-Path $HOME '.lbterm.lua'), (Join-Path $HOME '.config\lbterm\lbterm.lua')) {
    if (Test-Path -LiteralPath $c) { return $c }
  }
  throw "No LBTerm config for a test window to look like: name one as `"config`" in $dev, or write ~\.lbterm.lua"
}

# A config file for a test window that looks like Lachlan's LBTerm: his lbterm.lua, without
# what reaches outside the window. No summon key, which a window in
# the background holds with RegisterHotKey, so his PageUp would go to the test window while it
# runs; no mux domain; and no Conductor, whose pad would move the rail's tabs down a slot and whose
# window would subscribe to his real hub (perf/conductor.ps1 gives its window one of its own); no
# InMail button, which would count his real InMail (perf/inbox.ps1 gives its window folders of its
# own, and his `lbterm-inbox-open` handler leaves alone a message from a folder that isn't his); no
# agenda button, which would show his real list and watch its folder, and whose presses would run
# his real list's commands (perf/agenda.ps1 gives its window a file of its own, and the button sits
# in the strip where other tests read it); no file for the new-tab picker's recent picks, which would
# be his (the picks stay in the window's memory; his lbterm-project-action and lbterm-reopen-tab
# handlers step aside in a test window themselves). And no reloading: WezTerm watches a config's folder (all but the home folder) as well as its
# files, so captures saved beside the config, a state file written there, or Lachlan saving
# lbterm.lua would reload the window mid-run, re-running his whole config and putting "Config
# reloaded" in the strip. A test of reloading turns it back on and gives its config a folder of
# its own (perf/strip-feedback.ps1).
# His lbterm_scroll_strip stays as he has it ({}), so the conversations' reader is on and reads his
# real ~/.claude/projects: a script that sets a claude_session on a pane gives
# lbterm_scroll_strip a transcripts folder of its own (perf/questions.ps1, PITFALLS 362).
# -Lua adds lines after it, as `config.x = y`, with `wezterm` in scope. Pass the file with
# `--config-file`, which also keeps ~\.lbterm.lua from loading.
function New-TestConfig {
  param([Parameter(Mandatory)][string]$Path, [string]$Lua = '')
  $lbtermLua = (Get-LookalikeConfig) -replace '\\', '/'
  [IO.File]::WriteAllText($Path, @"
local wezterm = require 'wezterm'
local config = dofile('$lbtermLua')
config.lbterm_summon = nil
config.unix_domains = nil
config.default_gui_startup_args = nil
config.lbterm_crash_restore = false
-- No WSL domains, whatever lbterm.lua says: unset, every spawn runs wsl.exe, which opens WSL's installer in front
config.wsl_domains = {}
rawset(config, 'lbterm_conductor', nil)
rawset(config, 'lbterm_inbox', nil)
rawset(config, 'lbterm_agenda', nil)
rawset(config, 'lbterm_context', nil)
rawset(config, 'lbterm_close_marks', nil)
rawset(config, 'lbterm_links', nil)
-- The new-tab picker's recent picks are his: a test window's stay in its memory (lbterm/NEW-TAB.md)
if rawget(config, 'lbterm_projects') then config.lbterm_projects.recent_file = nil end
config.automatically_reload_config = false
$Lua
return config
"@)
}
