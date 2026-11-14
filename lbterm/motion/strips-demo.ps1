#Requires -Version 7
<#
Records the context bar, the cost pane and the scroll strip at work (lbterm/CONTEXT.md,
lbterm/SCROLL-STRIP.md): a demo for the portfolio page, staged from end to end, so nothing in it is
one of Lachlan's real sessions. Until 2026-11-14 it recorded the objectives strip, where the bar
is now.

  pwsh -File lbterm\motion\strips-demo.ps1              # demo.gif and the frames
  pwsh -File lbterm\motion\strips-demo.ps1 -Live        # the window left open, nothing played

Opens target\release\lbterm-gui.exe (or -Bin's) looking like Lachlan's LBTerm (New-TestConfig)
with the tab panel, the context bar (`lbterm_context_bar`, in the objectives strip's room), the
scroll strip and lbterm_summaries pointed at folders of the demo's own. Five tabs hold made-up
sessions in the projects the portfolio page names, each with a label on the rail. The open tab,
Rat drops, runs perf/fake-claude.ps1 with a made-up conversation (-Conversation) beside the
transcript it matches, so its scroll strip is the conversation's: six prompts, three edits, a
failed test run, two questions, a permission prompt, two commits, an InMail and a subagent. The
transcript also has what the bar reads, as perf/context-bar.ps1 writes it: the files loaded at the
start (a user CLAUDE.md importing a rule, two project CLAUDE.md files, the memory index), the
skills, the first request's system prompt, tools and messages, each reply's usage growing with the
conversation, a memory file read and another project's CLAUDE.md pulled in as it's told about the
tails; its usage.json says $1.84 of a 200k window, and its cost.json is a made-up split in the
Claude project's shape (his turns and the tutorial's subagent), which the cost pane shows with
LBTerm's own count off, his setting. The other four tabs are demo-tab.ps1, each with its pad's
state.

Then a pointer, posted, never the real one: it glides onto the bar's conversation section (the bar
widens with each section's number, and its pane of files comes out from under it), down to the
memory file it read, off into the pane under it (both go back), presses the context pill's dollars
(the cost pane opens under the tab bar: the split, its table and the graph over time), off again
(it closes), onto the scroll strip's failed test run and its first prompt (each mark's card),
clicks the first prompt (the stand-in Claude Code scrolled back to it by the strip's own PgUps,
its row marked, the thumb up the track), then the last prompt's mark to go back to the latest.

Frames go to target\motion\strips\frames (frame000.png..., frames.csv: frame, ms, and the
pointer's x and y in the client area, -1 while it's off the window), every -StepMs as near as the
captures allow, cropped to the client area; gif.py --pointer draws an arrow where the pointer was
and makes demo.gif at real speed. Needs Python with Pillow. Opens with Start-Unfocused on the
right-most screen and never takes focus; its USERPROFILE is a scratch folder, so lbterm.lua's
handlers write nothing real. Run it inside a warden job, as any motion script:

  warden-run --name lbterm-perf --cpus 4 --mem 8 --gpu-draw --lock lbterm-measure --max 30m -- pwsh -NoProfile -File <tree>\lbterm\motion\strips-demo.ps1 -Bin <build>
#>
param(
  [int]$Dpi = 120,
  [int]$Cols = 84,
  [int]$Rows = 28,
  [int]$StepMs = 33,
  [switch]$Live,
  [switch]$Keep,
  [string]$Bin
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
if ($Live) { $Keep = $true }
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$out = Join-Path $repo 'target\motion\strips'
$T = Start-Test 'motion-strips' -Bin $Bin -Out $out -Class 'com.lachlanb.lbterm.strips-demo'
$home2 = Join-Path $out 'home'
$sums = Join-Path $out 'summaries'
$projects = Join-Path $out 'projects'
$folder = Join-Path $projects 'C--work-spirescape'
$frames = Join-Path $out 'frames'
New-Item -ItemType Directory -Force $home2, $sums, $folder, $frames | Out-Null

# The sessions: made-up work in the projects the portfolio page names, nothing of Lachlan's own.
# The first is the open tab, the stand-in Claude Code with the conversation below
$sessions = [ordered]@{
  spirescape = @{ sid = 'd0d0d0d0-0000-4000-8000-000000000001'; label = 'Rat drops'; state = 'waiting'; age = 1 }
  website = @{ sid = 'd0d0d0d0-0000-4000-8000-000000000002'; label = 'Sea heath'; state = 'waiting'; age = 4 }
  gloomfell = @{ sid = 'd0d0d0d0-0000-4000-8000-000000000003'; label = 'Mage v4'; state = 'working'; age = 0 }
  qualitylab = @{ sid = 'd0d0d0d0-0000-4000-8000-000000000004'; label = 'Pytest run'; state = 'working'; age = 0 }
  lbterm = @{ sid = 'd0d0d0d0-0000-4000-8000-000000000005'; label = 'Rail labels'; state = 'waiting'; age = 22 }
}
foreach ($k in $sessions.Keys) {
  [IO.File]::WriteAllText((Join-Path $sums "$($sessions[$k].sid).label.txt"), $sessions[$k].label)
}

# The open tab's conversation, twice: the transcript the strip reads (as scroll-strip.ps1 writes
# one) and the rows fake-claude.ps1 draws for it, block by block, a blank row before each
$start = [DateTime]::new(2026, 11, 10, 9, 0, 0, [DateTimeKind]::Local).ToUniversalTime()
$records = [Collections.Generic.List[string]]::new()
$screen = [Collections.Generic.List[object]]::new()
$script:second = 0
$commitAt = 0
$script:inUse = 27000
function Get-Usage([int]$grow) {
  $script:inUse += $grow
  @{ input_tokens = 2; cache_read_input_tokens = $script:inUse - 1002; cache_creation_input_tokens = 1000; output_tokens = 120 }
}
function Add-Record([hashtable]$r, [int]$after = 6) {
  $script:second += $after
  $r.timestamp = $start.AddSeconds($script:second).ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
  $r.isSidechain = $false
  $records.Add(($r | ConvertTo-Json -Compress -Depth 8))
}
function Row([string]$Kind, [string]$Text = '') { $screen.Add([ordered]@{ kind = $Kind; text = $Text }) }
function Ask([string]$Text) {
  Add-Record @{ type = 'user'; origin = @{ kind = 'human' }; message = @{ role = 'user'; content = $Text } } 40
  Row blank; Row user $Text
}
function Say([string[]]$Words) {
  Add-Record @{ type = 'assistant'; message = @{ role = 'assistant'; model = 'claude-opus-5-5'; usage = (Get-Usage 1500); content = @(@{ type = 'text'; text = ($Words -join "`n") }) } }
  Row blank; Row first $Words[0]
  foreach ($w in $Words | Select-Object -Skip 1) { Row text $w }
}
function Tool([string]$Id, [string]$Name, [hashtable]$Tool, [string]$Header, [string]$Result, [switch]$Failed, [string]$Sent = $Result, [int]$Grow = 2500) {
  Add-Record @{ type = 'assistant'; message = @{ role = 'assistant'; model = 'claude-opus-5-5'; usage = (Get-Usage $Grow); content = @(@{ type = 'tool_use'; id = $Id; name = $Name; input = $Tool }) } }
  Add-Record @{ type = 'user'; message = @{ role = 'user'; content = @(@{ type = 'tool_result'; tool_use_id = $Id; content = $Sent; is_error = [bool]$Failed }) } } 9
  Row blank; Row $(if ($Failed) { 'error' } else { 'tool' }) $Header; Row result $Result
}
function Turn([int]$Seconds) {
  Add-Record @{ type = 'system'; subtype = 'turn_duration'; durationMs = $Seconds * 1000 } 1
  $done = $start.AddSeconds($script:second).ToLocalTime().ToString('H:mm')
  Row blank; Row turn "Worked for ${Seconds}s $([char]0xb7) done $done"
}

# What Claude Code shows above the conversation and under its input box, made up to match
foreach ($t in '', " $([char]0x273b)  Claude Code", '    Opus', '    ~\work\spirescape', '') { Row banner $t }
foreach ($t in "  $([char]0x25b8) Rat drop table: bones every kill, a tail now and then", '  opus | spirescape | main', '  $1.84 so far') { Row status $t }

Ask 'rats should drop bones every kill, and a tail now and then'
$userDir = 'C:/Users/dev/.claude'
$memory = "$userDir/projects/C--work-spirescape/memory"
Add-Record @{ type = 'attachment'; attachment = @{ type = 'instructions'; files = @(
  @{ path = "$userDir/CLAUDE.md"; type = 'User'; content = ('u' * 9000) + "`n@$userDir/rules/asking.md`n" }
  @{ path = "$userDir/rules/asking.md"; type = 'User'; content = 'a' * 2400 }
  @{ path = 'C:/work/CLAUDE.md'; type = 'Project'; content = 'p' * 12000 }
  @{ path = 'C:/work/spirescape/CLAUDE.md'; type = 'Project'; content = 'h' * 6000 }
  @{ path = "$memory/MEMORY.md"; type = 'AutoMem'; content = 'm' * 1600 }
) } } 0
Add-Record @{ type = 'attachment'; attachment = @{ type = 'skill_listing'; isInitial = $true; content = 'x' }; rendered = @(@{ content = 'k' * 3000 }) } 0
$raw = { param($r) $records.Add(($r | ConvertTo-Json -Compress -Depth 10)) }
& $raw @{ type = 'api-request-shape'; shapeHash = 's1'; shape = @{ system = @(@{ type = 'text'; text = 's' * 12000 }); tools = @(@{ name = 'Read'; description = 'd' * 36000 }) } }
& $raw @{ type = 'api-request-blob'; hash = 'm1'; message = @{ role = 'user'; content = @(@{ type = 'text'; text = 'b' * 40000 }) } }
& $raw @{ type = 'api-request'; querySource = 'repl_main_thread'; shapeHash = 's1'; keep = 0; tail = @('m1') }
Say "I'll add both to the rat's table in content/drops/rat.json."
Tool 'e1' 'Edit' @{ file_path = 'C:\work\spirescape\content\drops\rat.json' } 'Update(content\drops\rat.json)' 'Added 2 lines'
Tool 'b1' 'Bash' @{ command = 'dotnet test' } 'Bash(dotnet test)' 'Failed: DropTableTests.RatesAddUpToOne' -Failed
Say @('The content check wants a table''s rates to add up to 1, and bones at',
  '1.0 with the tail at 0.05 come to 1.05.',
  'Should an always-drop sit outside the rates, or should I scale the rest?')
Turn 41

Ask 'rates are per item, not shares of one roll. fix the check, not the table'
Say @('Right: each item rolls on its own. I''ll hold each rate between 0 and 1',
  'instead of making them add up to 1.')
Tool 'e2' 'Edit' @{ file_path = 'C:\work\spirescape\src\Rules\DropTableCheck.cs' } 'Update(src\Rules\DropTableCheck.cs)' 'Added 3 lines, removed 5 lines'
Tool 'b2' 'Bash' @{ command = 'dotnet test' } 'Bash(dotnet test)' 'Passed: 212, failed: 0'
Tool 'b3' 'Bash' @{ command = 'git commit -am "Rats drop bones every kill and a tail now and then"' } 'Bash(git commit -am "Rats drop bones every kill and a tail...")' '[main 4c1e9a2] 2 files changed, 9 insertions(+), 5 deletions(-)'
$commitAt = [DateTimeOffset]::new($start.AddSeconds($script:second - 9)).ToUnixTimeMilliseconds()
Say 'Committed. All 212 tests pass.'
Turn 63

Ask 'does a miss still drop bones?'
Say @('No. A miss ends the fight without a kill, so nothing drops. Bones on a',
  'miss would need a rule of its own in Combat.cs.')
Turn 9

Ask 'yes, on a miss too. it''s a rat, it leaves bones either way'
Say 'I''ll give a miss its own rule: it drops the table''s always-items only.'
Tool 'r1' 'Read' @{ file_path = "$memory/feedback_drop_rules.md" } 'Read(memory\feedback_drop_rules.md)' 'Read 24 lines' -Sent ('r' * 2400) -Grow 1200
Tool 'e3' 'Edit' @{ file_path = 'C:\work\spirescape\src\Rules\Combat.cs' } 'Update(src\Rules\Combat.cs)' 'Added 6 lines'
Tool 'b4' 'Bash' @{ command = 'dotnet test' } 'Bash(dotnet test)' 'Passed: 214, failed: 0'
Tool 'b5' 'Bash' @{ command = 'git commit -am "A miss still drops a rat''s bones"' } 'Bash(git commit -am "A miss still drops a rat''s bones")' '[main 7b3d0f5] 3 files changed, 18 insertions(+)'
Say 'Done and committed, with two new tests for a miss.'
Turn 52

Ask 'tell gloomfell rats drop tails now, the cook''s quest will want them'
$ledger = 'C:/work/gloomfell/CLAUDE.md'
Add-Record @{ type = 'attachment'; attachment = @{ type = 'nested_memory'; path = $ledger; displayPath = 'CLAUDE.md'; content = @{ path = $ledger; type = 'Project'; content = 'n' * 14000 } } } 1
Tool 'b6' 'Bash' @{ command = 'inmail send GloomFell "Rats drop tails now" --body -' } 'Bash(inmail send GloomFell "Rats drop tails now")' 'Sent to GloomFell'
Say 'Sent. It says a rat drops a tail about one kill in twenty.'
Turn 14

Ask 'play the tutorial through in the background and tell me if it breaks'
Tool 'a1' 'Agent' @{ description = 'Play the tutorial through'; subagent_type = 'general-purpose' } 'general-purpose(Play the tutorial through)' 'Running in the background'
Say @('It''s running. A headless run takes about four minutes.',
  'Should I file anything it finds as a bug, or just tell you?')
Turn 11

$sid = $sessions.spirescape.sid
[IO.File]::WriteAllText((Join-Path $folder "$sid.jsonl"), ($records -join "`n") + "`n")
# The permission prompt on the first commit, as claudehook records one
[IO.File]::WriteAllText((Join-Path $sums "$sid.events.jsonl"), "{`"at`":$($commitAt + 300),`"event`":`"PermissionRequest`",`"tool`":`"Bash`"}`n")
$conversation = Join-Path $out 'conversation.json'
[IO.File]::WriteAllText((Join-Path $sums "$sid.usage.json"), '{"usd":1.84,"window":200000}')
$t0 = [DateTimeOffset]::new($start).ToUnixTimeMilliseconds()
$minutes = [int][Math]::Ceiling($script:second / 60)
$steps = (0..($minutes - 1) | ForEach-Object { '[{0},{1}]' -f ($t0 + $_ * 60000), ([Math]::Round(1.72 * ($_ + 1) / $minutes, 2)).ToString([Globalization.CultureInfo]::InvariantCulture) }) -join ','
$steps += ',[{0},1.84]' -f ($t0 + $minutes * 60000)
$agentAt = $t0 + ($script:second - 20) * 1000
$split = '{{"v":1,"usd":1.84,"spenders":[' +
  '{{"kind":"main","id":"{0}","run":null,"label":null,"start":{1},"end":{2},"requests":24,"usd":1.72,"tokens":[180,64000,4800000,22000],"dollars":[0.0007,0.32,0.96,0.4393]}},' +
  '{{"kind":"subagent","id":"atutorial","run":null,"label":"Play the tutorial through","start":{3},"end":{4},"requests":6,"usd":0.12,"tokens":[40,12000,150000,1500],"dollars":[0.0002,0.06,0.03,0.0298]}}' +
  '],"compactions":[],"steps":[{5}]}}'
[IO.File]::WriteAllText((Join-Path $sums "$sid.cost.json"), ($split -f $sid, $t0, ($t0 + $script:second * 1000), $agentAt, ($agentAt + 240000), $steps))
[IO.File]::WriteAllText($conversation, (ConvertTo-Json -InputObject @($screen) -Depth 3))

# The open tab: its pad's light as claudehook.exe sets it, then the stand-in Claude Code
$fake = Join-Path $repo 'lbterm\perf\fake-claude.ps1'
$openTab = New-PwshProgram @"
`$stamp = [DateTime]::UtcNow.AddMinutes(-$($sessions.spirescape.age)).ToFileTimeUtc()
`$b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("ff8c00|`$stamp"))
[Console]::Write("`$([char]27)]1337;SetUserVar=claude_status=`$b64`$([char]7)")
& '$fake' -Sid '$sid' -Conversation '$conversation'
"@

$lua = @"
config.lbterm_compositor = true
config.lbterm_tab_panel = { enabled = true }
config.lbterm_motion = { enabled = 'on' }
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
rawset(config, 'lbterm_context', { transcripts = '$(ConvertTo-LuaPath $projects)' })
rawset(config, 'lbterm_context_bar', {})
-- The projects pad's card lists the demo's made-up projects, never this PC's
rawset(config, 'lbterm_projects', { projects = {
  { name = 'SpireScape', dir = [[C:\work\spirescape]] },
  { name = 'GloomFell', dir = [[C:\work\gloomfell]] },
  { name = 'Website', dir = [[C:\work\website]] },
  { name = 'QualityLab', dir = [[C:\work\qualitylab]] },
  { name = 'LBTerm', dir = [[C:\work\lbterm]] },
} })
config.lbterm_scroll_strip = { transcripts = '$(ConvertTo-LuaPath $projects)' }
config.dpi = $Dpi
config.initial_cols = $Cols
config.initial_rows = $Rows
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
"@

$demoTab = Join-Path $PSScriptRoot 'demo-tab.ps1'
try {
  $w = Open-TestWindow -Lua $lua -UserProfile $home2 -WorkingDirectory $repo -PostedPointer -Program $openTab
  Wait-State { param($s) $s.frame -gt 0 -and $s.rail -and $s.context_bar -and $s.scroll_strip } -What 'the window, its rail, the context bar and the scroll strip' | Out-Null
  $first = (Get-Panes)[0].pane_id
  foreach ($k in @($sessions.Keys) | Select-Object -Skip 1) {
    $s = $sessions[$k]
    $a = @('-Title', $s.label, '-State', $s.state, '-AgeMinutes', $s.age, '-Session', $s.sid)
    # Not renamed: a tab's title would take the place of its label on the rail
    [void](New-Tab -From $first -NoActivate -Program (New-PwshProgram -File $demoTab -ArgumentList $a))
  }
  $prompts = { param($s) @($s.scroll_strip.marks | Where-Object kind -eq 'prompt' | Sort-Object { $_.rect.y }) }
  $st = Wait-State { param($s)
    @($s.rail.rows).Count -eq 5 -and $s.context_bar.session -eq $sid -and -not $s.context_bar.waiting -and @($s.context_bar.sections).Count -ge 10 -and $s.cost.pill -and
    $s.scroll_strip.mode -eq 'claude' -and $s.scroll_strip.thumb -and (& $prompts $s).Count -eq 6 } -What 'five rows, the context bar''s sections, the conversation on the scroll strip' -TimeoutSec 30
  "context bar: $($st.context_bar.in_use) of $($st.context_bar.window) in use; $((@($st.context_bar.sections) | ForEach-Object { "$($_.kind) $($_.tokens)" }) -join ', ')"
  "scroll strip: $((@($st.scroll_strip.marks) | Group-Object kind | ForEach-Object { "$($_.Count) $($_.Name)" }) -join ', ')"
  if ($Live) {
    "Strips demo window open (pid $($w.Pid))"
    return
  }
  # New tabs grow in and their marks fade in: all of that over before the recording starts
  Start-Sleep 2
  $st = Wait-Settled -What 'the window to settle'

  # Where the pointer goes, from the state file (client pixels, as posted messages take them)
  $pane = @($st.panes)[0]
  $cell = $pane.rect.h / $pane.rows
  $bar = $st.context_bar
  $bx = [int]($bar.rect.x + $bar.rect.w / 2)
  $section = { param($kind) $r = @($bar.sections | Where-Object { $_.kind -eq $kind -and $_.rect })[0].rect; if (-not $r) { throw "no $kind section on the bar" }; [int]($r.y + $r.h / 2) }
  $pill = Get-RectCenter $st.cost.pill
  $track = $st.scroll_strip.track
  $sx = [int]($track.x + $track.w / 2)
  $mid = { param($m) [int]($m.rect.y + $m.rect.h / 2) }
  $p = & $prompts $st
  $err = @($st.scroll_strip.marks | Where-Object kind -eq 'error')[0]
  if (-not $err) { throw 'no error mark on the scroll strip' }
  $low = [int]($track.y + $track.h + 2.5 * $cell)
  $inX = [int]($pane.rect.x + $pane.rect.w * 0.8)
  # Low in the pane, under the cost pane and left of the bar's pane: resting there shuts both
  $restX = [int]($pane.rect.x + $pane.rect.w * 0.25)
  $restY = [int]($pane.rect.y + $pane.rect.h * 0.88)
  # Keyframes: at ms the pointer is at (x, y), gliding between them (eased); $null is off the window
  # A rehearsal before the recording: widened, the bar lays its sections out again (a line each at
  # least, CONTEXT.md, "The pointer"), so the memory file's place is read from the bar widened, not
  # from the bar at rest, where its y is another section's once it widens
  $convY = & $section 'conversation'
  Send-Mouse $w Move $bx $convY
  $open = Wait-State { param($s) $s.context_bar.widened -and $s.context_bar.pane -and -not $s.animating } -What 'the bar widened, its pane out' -TimeoutSec 10
  $mi = [array]::IndexOf(@($open.context_bar.sections | ForEach-Object kind), 'memory')
  $ml = @($open.context_bar.widened.labels | Where-Object section -eq $mi)[0]
  if (-not $ml) { throw 'no number on the widened bar for the memory file' }
  $memY = [int]($ml.rect.y + $ml.rect.h / 2)
  Send-Mouse $w Move $restX $restY
  Wait-State { param($s) -not $s.context_bar.widened -and -not $s.context_bar.pane -and -not $s.animating } -What 'the bar back at rest' -TimeoutSec 10 | Out-Null
  Send-Leave $w
  $st = Wait-Settled -What 'the window to settle after the rehearsal'
  # The pill is at the tab bar's left, the rail under it and the projects pad right of it: a glide
  # that slows over the rail opens the tab panel over the cost pane, and one along the tab bar
  # slows on the pad and opens the projects card, which lists this PC's real projects (it isn't
  # staged). So the pointer goes up to the title's row, along it, and down onto the pill, and back
  # the same way
  $top = [int]($st.cost.pill.y / 2)
  $keys = @(
    @(0, $null), @(700, $inX, $low),
    # Onto the bar's conversation: it widens with each section's number, and its pane of files
    # comes out; then down to the memory file it read, its row lit
    @(1200, $bx, $convY), @(4000, $bx, $convY), @(4500, $bx, $memY), @(6000, $bx, $memY),
    # Off it, so both go back; then a press on the dollars opens the cost pane
    @(6600, $restX, $restY), @(7200, $restX, $restY), @(7700, $restX, $top), @(8100, $pill[0], $top), @(8400, $pill[0], $pill[1]),
    @(11200, $pill[0], $pill[1]), @(11500, $pill[0], $top), @(11900, $restX, $top), @(12400, $restX, $restY), @(12700, $restX, $restY),
    # The scroll strip: the failed test run's mark, the first prompt's (clicked), the last's (clicked)
    @(13300, $sx, (& $mid $err)), @(14700, $sx, (& $mid $err)),
    @(15100, $sx, (& $mid $p[0])), @(18100, $sx, (& $mid $p[0])), @(18500, $sx, (& $mid $p[5])), @(20900, $sx, (& $mid $p[5])),
    @(21000, $null)
  )
  $clicks = @(@{ at = 8600; x = $pill[0]; y = $pill[1] }, @{ at = 15600; x = $sx; y = (& $mid $p[0]) }, @{ at = 18900; x = $sx; y = (& $mid $p[5]) })
  $endMs = 21500
  $pointerAt = { param([double]$t)
    $k = 0
    while ($k + 1 -lt $keys.Count -and $keys[$k + 1][0] -le $t) { $k++ }
    $a = $keys[$k]
    if ($a.Count -lt 3) { return $null }
    if ($k + 1 -ge $keys.Count -or $keys[$k + 1].Count -lt 3) { return @($a[1], $a[2]) }
    $b = $keys[$k + 1]
    $f = ($t - $a[0]) / ($b[0] - $a[0]); $f = $f * $f * (3 - 2 * $f)
    @([int]($a[1] + ($b[1] - $a[1]) * $f), [int]($a[2] + ($b[2] - $a[2]) * $f))
  }

  # Frames every $StepMs, saved as they're taken: a whole run's bitmaps don't fit in memory
  $h = Resolve-Handle $w
  $csv = [Collections.Generic.List[string]]::new(); $csv.Add('frame,ms,x,y')
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $n = 0; $nextClick = 0; $was = $null
  while ($clock.ElapsedMilliseconds -lt $endMs) {
    $t = $clock.Elapsed.TotalMilliseconds
    $at = & $pointerAt $t
    if ($at -and (-not $was -or $at[0] -ne $was[0] -or $at[1] -ne $was[1])) { Send-Mouse $w Move $at[0] $at[1] }
    elseif (-not $at -and $was) { Send-Leave $w }
    $was = $at
    while ($nextClick -lt $clicks.Count -and $t -ge $clicks[$nextClick].at) { Send-Click $w $clicks[$nextClick].x $clicks[$nextClick].y; $nextClick++ }
    $shot = [LbtermE2E.Win]::Capture($h)
    $ms = [Math]::Round($clock.Elapsed.TotalMilliseconds, 1)
    if ($shot) {
      $shot.Save((Join-Path $frames ('frame{0:D3}.png' -f $n)))
      $shot.Dispose()
      $xy = if ($at) { "$($at[0]),$($at[1])" } else { '-1,-1' }
      $csv.Add("$n,$ms,$xy"); $n++
    }
    $wait = [int]($n * $StepMs - $clock.ElapsedMilliseconds)
    if ($wait -gt 0) { Start-Sleep -Milliseconds $wait }
  }
  Watch-Foreground
  $csv | Set-Content (Join-Path $frames 'frames.csv')
  $end = Wait-State { $true } -What 'the state'
  "$n frames in $([Math]::Round($endMs / 1000, 1)) s to $frames (DPI scale $($w.Scale)); the view at $($end.scroll_strip.view_top) at the end"
} finally {
  Close-Test -Keep:$Keep
}
if ($Live) { return }
python (Join-Path $PSScriptRoot 'gif.py') $frames --out (Join-Path $out 'demo.gif') --step 40 --hold 400 --pointer --pointer-scale ($Dpi / 96)
