#Requires -Version 7
<#
The Conductor's tab for conductor-demo.ps1: a stand-in for its Claude Code session, drawn on the
alternate screen as Claude Code draws, and told what to show by lines of JSON appended to -Cues:

  {"type":"type","text":"..."}      Lachlan types into the input box, a key every 30 ms
  {"type":"send","spin":"..."}      what he typed goes up as his message; the spinner starts
  {"type":"tool","id":"t1","name":"read","arg":"website-a6"}
                                    a tool call, its bullet blinking while it runs
  {"type":"result","id":"t1","text":"..."}
                                    the call done (a green bullet) and its result under it
  {"type":"group","ids":["t1","t2"],"text":"Read 2 sessions"}
                                    calls done together, folded into one line as Claude Code
                                    folds parallel reads
  {"type":"say","lines":["...","..."]}
                                    the answer, streamed in; the spinner stops

It sets the pane's claude_session user var, as claudehook.exe does, so the Conductor's pad
knows this tab for its own.

  pwsh -NoProfile -File conductor-tab.ps1 -Sid <session id> -Cues <file>
#>
param(
  [Parameter(Mandatory)][string]$Sid,
  [Parameter(Mandatory)][string]$Cues,
  [string]$Status = 'read only'
)
$ErrorActionPreference = 'Stop'
$e = [char]27
[Console]::OutputEncoding = [Text.Encoding]::UTF8
[Console]::Write("$e]1337;SetUserVar=claude_session=$([Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Sid)))$([char]7)")
# The alternate screen, no cursor of the terminal's own: the input box draws its own
[Console]::Write("$e[?1049h$e[?25l")

function Rgb([int]$r, [int]$g, [int]$b) { "$e[38;2;${r};${g};${b}m" }
function Bg([int]$r, [int]$g, [int]$b) { "$e[48;2;${r};${g};${b}m" }
$reset = "$e[0m"
$bold = "$e[1m"
$orange = Rgb 215 119 87
$dim = Rgb 140 140 140
$white = Rgb 230 230 230
$green = Rgb 78 186 101
$frame = Rgb 110 110 110

# The conversation: entries of kind user, tool, group or say
$entries = [Collections.Generic.List[object]]::new()
$tools = @{}
$typed = ''
$typing = [Collections.Generic.Queue[char]]::new()
$spin = $null
$spinSince = [DateTime]::Now
$streaming = $null
$clock = [Diagnostics.Stopwatch]::StartNew()

# Words wrapped to $width, the first line after $first and the rest after $rest
function Wrap([string]$text, [int]$width, [string]$first, [string]$rest) {
  $out = [Collections.Generic.List[string]]::new()
  $line = ''
  foreach ($word in $text -split ' ') {
    if ($line.Length -and ($line.Length + 1 + $word.Length) -gt $width) { $out.Add($line); $line = $word }
    elseif ($line.Length) { $line += " $word" }
    else { $line = $word }
  }
  $out.Add($line)
  for ($i = 0; $i -lt $out.Count; $i++) { $out[$i] = $(if ($i) { $rest } else { $first }) + $out[$i] }
  $out
}

function Render {
  $w = [Console]::WindowWidth
  $h = [Console]::WindowHeight
  $inner = $w - 4
  $rows = [Collections.Generic.List[string]]::new()
  foreach ($en in $entries) {
    if ($rows.Count) { $rows.Add('') }
    switch ($en.Kind) {
      'user' {
        $text = "$([char]0x276f) $($en.Text)"
        $rows.Add("$(Bg 55 55 55)$white$text$(' ' * [Math]::Max(0, $w - 1 - $text.Length))$reset")
      }
      'tool' {
        # Running: the bullet blinks, as Claude Code's does
        $bullet = if ($en.Done) { $green } elseif ([int]($clock.ElapsedMilliseconds / 300) % 2) { $white } else { $dim }
        $arg = if ($en.Arg) { "($($en.Arg))" } else { '' }
        $rows.Add("$bullet$([char]0x25cf)$reset $bold$white$($en.Name)$reset$white$arg$reset")
        if ($en.Result) { $rows.Add("  $dim$([char]0x23bf)  $($en.Result)$reset") }
      }
      'group' {
        $rows.Add("$green$([char]0x25cf)$reset $white$($en.Text)$reset $dim(ctrl+o to expand)$reset")
      }
      'say' {
        $shown = $en.Shown
        $first = $true
        foreach ($para in $en.Lines) {
          if ($shown -le 0) { break }
          $part = $para.Substring(0, [Math]::Min($para.Length, $shown))
          $shown -= $para.Length + 1
          foreach ($l in (Wrap $part ($w - 3) '' '')) {
            $lead = if ($first) { "$white$([char]0x25cf)$reset " } else { '  ' }
            $rows.Add("$lead$white$l$reset")
            $first = $false
          }
        }
      }
    }
  }
  # The header, the conversation's last rows, the spinner, the input box
  $top = @("$orange$bold$([char]0x273b) Conductor$reset  $dim$Status $([char]0x00b7) watching every session$reset", '')
  $spinRow = ''
  if ($spin) {
    $glyphs = @([char]0x00b7, [char]0x2722, [char]0x2733, [char]0x2736, [char]0x273b, [char]0x273d, [char]0x273b, [char]0x2736, [char]0x2733, [char]0x2722)
    $g = $glyphs[[int]($clock.ElapsedMilliseconds / 110) % $glyphs.Count]
    $secs = [int]([DateTime]::Now - $spinSince).TotalSeconds
    $spinRow = "$orange$g $spin$([char]0x2026)$reset $dim(${secs}s $([char]0x00b7) esc to interrupt)$reset"
  }
  $line = "> $typed"
  if ($line.Length -gt $inner - 1) { $line = $line.Substring($line.Length - $inner + 1) }
  $box = @(
    "$frame$([char]0x256d)$([string][char]0x2500 * ($w - 2))$([char]0x256e)$reset"
    "$frame$([char]0x2502)$reset $white$line$reset$e[7m $e[27m$(' ' * [Math]::Max(0, $inner - $line.Length - 1)) $frame$([char]0x2502)$reset"
    "$frame$([char]0x2570)$([string][char]0x2500 * ($w - 2))$([char]0x256f)$reset"
  )
  $room = [Math]::Max(0, $h - $top.Count - 2 - $box.Count)
  $body = @($rows | Select-Object -Last $room)
  $screen = @($top) + $body + @(@('') * ($room - $body.Count)) + @('', $spinRow) + $box
  $sb = [Text.StringBuilder]::new()
  for ($r = 0; $r -lt [Math]::Min($h, $screen.Count); $r++) { [void]$sb.Append("$e[$($r + 1);1H$e[2K$($screen[$r])") }
  [Console]::Write($sb.ToString())
}

# The cue file, read as it grows; a line is taken only once its newline is there
$fs = $null
$pending = ''
function Read-Cues {
  if (-not $script:fs) {
    if (-not (Test-Path $Cues)) { return @() }
    $script:fs = [IO.FileStream]::new($Cues, 'Open', 'Read', [IO.FileShare]'ReadWrite, Delete')
  }
  $buf = [byte[]]::new(65536)
  $text = ''
  while (($n = $script:fs.Read($buf, 0, $buf.Length)) -gt 0) { $text += [Text.Encoding]::UTF8.GetString($buf, 0, $n) }
  if (-not $text) { return @() }
  $all = $script:pending + $text
  $parts = $all -split "`n"
  $script:pending = $parts[-1]
  @($parts | Select-Object -SkipLast 1 | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json })
}

$lastSize = ''
$lastTick = -1
while ($true) {
  $dirty = $false
  foreach ($c in (Read-Cues)) {
    $dirty = $true
    switch ($c.type) {
      'type' { foreach ($ch in $c.text.ToCharArray()) { $typing.Enqueue($ch) } }
      'send' {
        while ($typing.Count) { $typed += $typing.Dequeue() }
        $entries.Add([pscustomobject]@{ Kind = 'user'; Text = $typed })
        $typed = ''
        $spin = $c.spin; $spinSince = [DateTime]::Now
      }
      'tool' {
        $t = [pscustomobject]@{ Kind = 'tool'; Name = $c.name; Arg = $c.arg; Done = $false; Result = $null }
        $tools[$c.id] = $t
        $entries.Add($t)
      }
      'result' { $t = $tools[$c.id]; $t.Done = $true; $t.Result = $c.text }
      'group' {
        $at = $entries.IndexOf($tools[$c.ids[0]])
        foreach ($id in $c.ids) { [void]$entries.Remove($tools[$id]) }
        $entries.Insert($at, [pscustomobject]@{ Kind = 'group'; Text = $c.text })
      }
      'say' {
        $spin = $null
        $streaming = [pscustomobject]@{ Kind = 'say'; Lines = @($c.lines); Shown = 0; Total = (@($c.lines) | Measure-Object Length -Sum).Sum + @($c.lines).Count }
        $entries.Add($streaming)
      }
      'spin' { $spin = $c.text; $spinSince = [DateTime]::Now }
    }
  }
  # A key every 30 ms; the answer about 160 characters a second, as Claude Code streams
  if ($typing.Count) { $typed += $typing.Dequeue(); $dirty = $true }
  if ($streaming -and $streaming.Shown -lt $streaming.Total) {
    $streaming.Shown += 5
    $dirty = $true
  }
  # The spinner turns and running bullets blink on their own ticks
  $tick = [int]($clock.ElapsedMilliseconds / 110)
  if ($tick -ne $lastTick -and ($spin -or ($tools.Values | Where-Object { -not $_.Done }))) { $dirty = $true }
  $lastTick = $tick
  $size = "$([Console]::WindowWidth)x$([Console]::WindowHeight)"
  if ($size -ne $lastSize) { [Console]::Write("$e[2J"); $lastSize = $size; $dirty = $true }
  if ($dirty) { Render }
  Start-Sleep -Milliseconds 30
}
