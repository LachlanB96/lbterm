# Pester 5+, run by `lbterm.ps1 check`. What `lbterm.ps1 logs` makes of LBTerm's logs: their lines
# and dates, which LBTerm wrote each, problems and slow frames grouped, and the views. On logs
# written under TestDrive the way env-bootstrap's ringlog.rs writes them; nothing here reads the
# real ~\.local\share\lbterm.

BeforeAll {
  . (Join-Path $PSScriptRoot '..\lbterm-lib.ps1')

  # A line as ringlog.rs writes it to the file: the time of day, the level padded to 6, the target
  # padded to the longest so far
  function Line([string]$Time, [string]$Level, [string]$Target, [string]$Message, [int]$Pad = 30) {
    '{0}  {1,-6} {2} > {3}' -f $Time, $Level, $Target.PadRight($Pad), $Message
  }

  # A log file of $Lines, made at $Created and last written at $Written. Its path.
  function New-Log {
    param([string]$Dir, [string]$Name, [string[]]$Lines, [datetime]$Created, [datetime]$Written)
    New-Item -ItemType Directory -Force $Dir | Out-Null
    $path = Join-Path $Dir $Name
    [IO.File]::WriteAllText($path, (($Lines | ForEach-Object { "$_`n" }) -join ''))
    $item = Get-Item $path
    $item.CreationTime = $Created
    $item.LastWriteTime = $Written
    $path
  }

  $install = 'C:\Users\Someone\AppData\Local\Programs\LBTerm'
  $day = [datetime]'2026-10-21'
}

Describe 'ConvertFrom-LbtermLog' {
  It 'reads the time, level, target and message of each line' {
    $text = (Line '10:00:01.250' 'INFO' 'lbterm_gui::termwindow::webgpu' 'drawing through DirectComposition'),
      (Line '10:00:02.000' 'ERROR' 'mux::ssh_agent' 'failed > to set') -join "`n"
    $e = @(ConvertFrom-LbtermLog $text $day.AddHours(10).AddSeconds(5))
    $e.Count | Should -Be 2
    $e[0].Level | Should -Be 'INFO'
    $e[0].Target | Should -Be 'lbterm_gui::termwindow::webgpu'
    $e[0].Message | Should -Be 'drawing through DirectComposition'
    $e[0].Time | Should -Be $day.AddHours(10).AddSeconds(1.25)
    $e[1].Message | Should -Be 'failed > to set'
    $e[1].Lines | Should -Be @($text -split "`n")[1]
  }

  It "keeps a message's further lines with it" {
    $text = (Line '10:00:01.000' 'ERROR' 'mux::tab' 'No space for split!!! SplitDirectionAndSize {'),
      '    direction: Horizontal,', '}', (Line '10:00:02.000' 'INFO' 'mux' 'next') -join "`n"
    $e = @(ConvertFrom-LbtermLog $text $day.AddHours(11))
    $e.Count | Should -Be 2
    $e[0].Message | Should -Be "No space for split!!! SplitDirectionAndSize {`n    direction: Horizontal,`n}"
    $e[0].Lines.Count | Should -Be 3
  }

  It 'reads CRLF lines and an empty log' {
    @(ConvertFrom-LbtermLog ((Line '10:00:01.000' 'WARN' 'a' 'b') + "`r`n") $day.AddHours(11))[0].Message | Should -Be 'b'
    @(ConvertFrom-LbtermLog '' $day).Count | Should -Be 0
  }

  It 'puts the lines before midnight on the day before' {
    $text = (Line '23:59:58.000' 'INFO' 'a' 'before'), (Line '00:00:03.000' 'INFO' 'a' 'after') -join "`n"
    $e = @(ConvertFrom-LbtermLog $text $day.AddSeconds(4))
    $e[0].Time | Should -Be $day.AddSeconds(-2)
    $e[1].Time | Should -Be $day.AddSeconds(3)
  }

  It "dates a last line written just before midnight on the day before the file's time" {
    $e = @(ConvertFrom-LbtermLog (Line '23:59:59.990' 'INFO' 'a' 'late') $day.AddMilliseconds(500))
    $e[0].Time | Should -Be $day.AddMilliseconds(-10)
  }

  It 'keeps lines a few ms out of order on one day' {
    $text = (Line '10:00:00.500' 'INFO' 'a' 'one'), (Line '10:00:00.480' 'INFO' 'b' 'two') -join "`n"
    $e = @(ConvertFrom-LbtermLog $text $day.AddHours(10).AddSeconds(1))
    $e[0].Time.Date | Should -Be $day
    $e[1].Time.Date | Should -Be $day
  }

  It 'puts a run days before, under the same pid, back where the file was made' {
    $text = (Line '15:00:01.000' 'INFO' 'a' 'first run'), (Line '15:00:02.000' 'INFO' 'a' 'first run'),
      (Line '22:00:00.000' 'INFO' 'a' 'second run') -join "`n"
    $e = @(ConvertFrom-LbtermLog $text $day.AddHours(22).AddSeconds(1) -Start $day.AddDays(-2).AddHours(15).AddSeconds(1))
    $e[0].Time | Should -Be $day.AddDays(-2).AddHours(15).AddSeconds(1)
    $e[1].Time | Should -Be $day.AddDays(-2).AddHours(15).AddSeconds(2)
    $e[2].Time | Should -Be $day.AddHours(22)
  }

  It 'leaves the dates alone when the file was made with its first line' {
    $text = (Line '15:00:01.000' 'INFO' 'a' 'one'), (Line '22:00:00.000' 'INFO' 'a' 'two') -join "`n"
    $e = @(ConvertFrom-LbtermLog $text $day.AddHours(22).AddSeconds(1) -Start $day.AddHours(15).AddSeconds(1))
    $e[0].Time | Should -Be $day.AddHours(15).AddSeconds(1)
  }
}

Describe 'Get-LbtermExeKind' {
  It 'takes the install folder for the daily LBTerm, whatever its case' {
    Get-LbtermExeKind "$install\lbterm-gui.exe" $install | Should -Be 'daily'
    Get-LbtermExeKind 'c:\users\someone\appdata\local\programs\lbterm\lbterm-mux-server.exe' $install | Should -Be 'daily'
  }

  It "takes a tree's target\try\bin for a try window" {
    Get-LbtermExeKind 'C:\Code\LBTerm\.claude\worktrees\x\target\try\bin\lbterm-gui.exe' $install | Should -Be 'try'
  }

  It 'takes anywhere else for a test' {
    Get-LbtermExeKind 'C:\Code\LBTerm\target\release\lbterm-gui.exe' $install | Should -Be 'test'
    Get-LbtermExeKind 'C:\Code\LBTerm\target\perf\spawn-old\lbterm-mux-server.exe' $install | Should -Be 'test'
    Get-LbtermExeKind "$install-previous\lbterm-gui.exe" $install | Should -Be 'test'
  }
}

Describe 'Get-LbtermLogEvidence' {
  It "knows a try window by its socket and server, though it runs Lachlan's config" {
    $text = (Line '21:52:55.463' 'WARN' 'wezterm_client::client' 'While connecting to Socket("C:/Code/LBTerm/.claude/worktrees/scrollbar/target/try/sock"): connecting to C:/Code/LBTerm/.claude/worktrees/scrollbar/target/try/sock.  Will try spawning the server.'),
      (Line '21:53:25.838' 'INFO' 'lbterm_gui::conductor_hub' 'conductor hub: listening on \\.\pipe\conductor-hub') -join "`n"
    $e = Get-LbtermLogEvidence $text
    $e.Kind | Should -Be 'try'
    $e.Reason | Should -Be 'names a try build: C:\Code\LBTerm\.claude\worktrees\scrollbar\target\try\sock'
  }

  It "knows a test window by a tree's target, though it ran the installed server" {
    $text = (Line '22:43:44.673' 'WARN' 'wezterm_client::client' 'Running: "C:/Users/Someone/AppData/Local/Programs/LBTerm/lbterm-mux-server.exe" "--config-file" "C:/Code/LBTerm/.claude/worktrees/bugs/target/perf/split-focus/split.lua" "--daemonize"')
    (Get-LbtermLogEvidence $text).Kind | Should -Be 'test'
  }

  It 'reads paths as Rust Debug writes them, backslashes doubled' {
    $text = Line '17:21:00.000' 'ERROR' 'lbterm' 'failed to connect to Socket("C:\\Code\\LBTerm\\target\\perf\\spawn-no-activate\\perfspawn.sock"): terminating'
    $e = Get-LbtermLogEvidence $text
    $e.Kind | Should -Be 'test'
    $e.Reason | Should -Match 'target\\perf\\spawn-no-activate\\perfspawn\.sock$'
  }

  It 'knows a test by the scratchpad, LBTERM_TEST, its perf pipe, or debug logging' {
    (Get-LbtermLogEvidence (Line '17:05:23.000' 'ERROR' 'lbterm' 'failed to connect to Socket("C:\\Users\\Someone\\AppData\\Local\\Temp\\claude\\x\\scratchpad\\probe-mux\\sock")')).Kind | Should -Be 'test'
    (Get-LbtermLogEvidence (Line '10:00:00.000' 'WARN' 'lbterm_gui::termwindow::gpu_lost' 'LBTERM_TEST_REMOVE_DEVICE: removing the GPU device')).Kind | Should -Be 'test'
    (Get-LbtermLogEvidence (Line '10:00:00.000' 'INFO' 'lbterm_gui::conductor_hub' 'conductor hub: listening on \\.\pipe\lbterm-perf-conductor-4242')).Kind | Should -Be 'test'
    $e = Get-LbtermLogEvidence ((Line '10:00:00.000' 'INFO' 'lbterm_gui::conductor_hub' 'conductor hub: listening on \\.\pipe\conductor-hub'),
      (Line '10:00:01.000' 'DEBUG' 'lbterm_gui::termwindow::render::tab_panel' 'tab panel: pointer in') -join "`n")
    $e.Kind | Should -Be 'test'
    $e.Reason | Should -Match 'debug level'
  }

  It "knows the daily LBTerm by the install folder, the daily server's socket or the conductor hub" {
    $e = Get-LbtermLogEvidence (Line '23:11:18.191' 'INFO' 'lbterm_gui::restart' 'C:\Users\Someone\AppData\Local\Programs\LBTerm\version.txt names Some("20261021-231036-1261bf46"), this is 20261021-220928-003cb73b: the restart pill shows')
    $e.Kind | Should -Be 'daily'
    $e.Reason | Should -Be 'names the installed build: C:\Users\Someone\AppData\Local\Programs\LBTerm\version.txt'
    (Get-LbtermLogEvidence (Line '21:04:26.000' 'ERROR' 'lbterm' 'failed to connect to Socket("C:\\Users\\Someone\\.local/share\\lbterm\\sock"): connecting to C:\Users\Someone\.local/share\lbterm\sock; terminating')).Kind | Should -Be 'daily'
    (Get-LbtermLogEvidence (Line '22:27:39.347' 'INFO' 'lbterm_gui::conductor_hub' 'conductor hub: listening on \\.\pipe\conductor-hub')).Kind | Should -Be 'daily'
  }

  It "doesn't take a GUI's own socket for the daily server's" {
    (Get-LbtermLogEvidence (Line '15:37:28.000' 'WARN' 'wezterm_client::client' 'While connecting to Socket("C:\\Users\\Someone\\.local/share\\lbterm\\gui-sock-18352")')).Kind | Should -Be 'unknown'
  }

  It 'says unknown when nothing names a build' {
    $text = (Line '23:48:14.191' 'INFO' 'lbterm_gui::termwindow::webgpu' 'lbterm_compositor: drawing through DirectComposition on NVIDIA GeForce RTX 4080 (Dx12)'),
      (Line '23:48:17.183' 'ERROR' 'wezterm_mux_server_impl::local' 'encoding PDU to client: writing pdu data buffer: An existing connection was forcibly closed by the remote host. (os error 10054)') -join "`n"
    (Get-LbtermLogEvidence $text).Kind | Should -Be 'unknown'
    (Get-LbtermLogEvidence '').Kind | Should -Be 'unknown'
  }
}

Describe 'Get-LbtermLogs' {
  BeforeAll {
    $dir = Join-Path $TestDrive 'lbterm'
    $t = [datetime]'2026-10-21 12:00'
    # The daily GUI, running from the install folder: nothing in its log says so
    New-Log $dir 'lbterm-gui.exe-log-100.txt' @(
      (Line '12:00:00.000' 'INFO' 'lbterm_gui::termwindow::webgpu' 'drawing'),
      (Line '12:05:00.000' 'ERROR' 'wezterm_mux_server_impl::local' 'encoding PDU to client: os error 10054'),
      (Line '12:06:00.000' 'WARN' 'wezterm_client::client' 'slowish')) -Created $t -Written $t.AddMinutes(6)
    # A test window that's gone
    New-Log $dir 'lbterm-gui.exe-log-200.txt' @(
      (Line '11:00:00.000' 'WARN' 'wezterm_client::client' 'Running: "C:/Code/LBTerm/target/release/lbterm-mux-server.exe" "--config-file" "C:/Code/LBTerm/target/perf/mux/mux.lua"')) -Created $t.AddHours(-1) -Written $t.AddHours(-1)
    # A mux server whose pid a cli has now
    New-Log $dir 'lbterm-mux-server.exe-log-300.txt' @(
      (Line '10:00:00.000' 'INFO' 'lbterm_mux_server' 'the last pane has gone; exiting')) -Created $t.AddHours(-2) -Written $t.AddHours(-2)
    # A cli whose pid a later cli has, which hasn't written to it
    New-Log $dir 'lbterm.exe-log-400.txt' @(
      (Line '09:00:00.000' 'ERROR' 'lbterm' 'failed to connect to Socket("C:\\Users\\Someone\\.local\\share\\lbterm\\sock"): terminating')) -Created $t.AddHours(-3) -Written $t.AddHours(-3)
    # What isn't a process's log: the daemonized servers' stderr, sockets, the panel's session
    foreach ($other in 'log', 'sock', 'gui-sock-100', 'session-com.lachlanb.lbterm.panel.json', 'wezterm-gui.exe-log-5.txt') {
      Set-Content (Join-Path $dir $other) 'x'
    }
    $live = @{
      100 = @{ Name = 'lbterm-gui'; Path = "$install\lbterm-gui.exe"; StartTime = $t.AddSeconds(-2) }
      300 = @{ Name = 'lbterm'; Path = "$install\lbterm.exe"; StartTime = $t }
      400 = @{ Name = 'lbterm'; Path = 'C:\Code\LBTerm\target\release\lbterm.exe'; StartTime = $t }
    }
    $logs = @(Get-LbtermLogs -Dir $dir -Install $install -Live $live)
    $byPid = @{}
    foreach ($l in $logs) { $byPid[$l.Pid] = $l }
  }

  It "lists every process's log and nothing else, newest first" {
    $logs.Name | Should -Be @('lbterm-gui.exe-log-100.txt', 'lbterm-gui.exe-log-200.txt', 'lbterm-mux-server.exe-log-300.txt', 'lbterm.exe-log-400.txt')
    $logs.Process | Should -Be @('gui', 'gui', 'mux', 'cli')
    $logs.Exe | Should -Be @('lbterm-gui', 'lbterm-gui', 'lbterm-mux-server', 'lbterm')
  }

  It 'counts errors and warnings' {
    $byPid[100].Errors | Should -Be 1
    $byPid[100].Warnings | Should -Be 1
    $byPid[100].Entries.Count | Should -Be 3
  }

  It "takes a running process's kind and start from the process" {
    $byPid[100].Alive | Should -BeTrue
    $byPid[100].Kind | Should -Be 'daily'
    $byPid[100].Reason | Should -Be "runs from $install\lbterm-gui.exe"
    $byPid[100].Started | Should -Be $t.AddSeconds(-2)
  }

  It "takes a gone one's from its log, and its start from when the file was made" {
    $byPid[200].Alive | Should -BeFalse
    $byPid[200].Kind | Should -Be 'test'
    $byPid[200].Started | Should -Be $t.AddHours(-1)
  }

  It "doesn't take another exe, or one started since the log was written, for its writer" {
    $byPid[300].Alive | Should -BeFalse
    $byPid[300].Kind | Should -Be 'unknown'
    $byPid[400].Alive | Should -BeFalse
    $byPid[400].Kind | Should -Be 'daily'
  }

  It 'reads a log its process still has open for writing' {
    $path = New-Log (Join-Path $TestDrive 'open') 'lbterm-gui.exe-log-500.txt' @((Line '12:00:00.000' 'INFO' 'a' 'held')) -Created $t -Written $t
    $writer = [IO.File]::Open($path, 'Append', 'Write', [IO.FileShare]'ReadWrite, Delete')
    try {
      $l = @(Get-LbtermLogs -Dir (Join-Path $TestDrive 'open') -Install $install)
      $l.Count | Should -Be 1
      $l[0].Entries[0].Message | Should -Be 'held'
    } finally { $writer.Dispose() }
  }

  It 'finds none in a folder that is not there' {
    @(Get-LbtermLogs -Dir (Join-Path $TestDrive 'nowhere') -Install $install).Count | Should -Be 0
  }

  It 'narrows them to a kind, and to those written since a time' {
    @(Select-LbtermLogs $logs -Kind 'test').Pid | Should -Be @(200)
    @(Select-LbtermLogs $logs -Since $t.AddMinutes(-90)).Pid | Should -Be @(100, 200)
    @(Select-LbtermLogs $logs -Kind 'daily' -Since $t).Pid | Should -Be @(100)
    @(Select-LbtermLogs $logs).Count | Should -Be 4
  }
}

Describe 'ConvertTo-LbtermSince' {
  It 'counts 45s, 30m, 2h and 1d back from now' {
    $now = [datetime]'2026-10-21 18:00'
    ConvertTo-LbtermSince '45s' $now | Should -Be $now.AddSeconds(-45)
    ConvertTo-LbtermSince '30m' $now | Should -Be $now.AddMinutes(-30)
    ConvertTo-LbtermSince '2h' $now | Should -Be $now.AddHours(-2)
    ConvertTo-LbtermSince ' 1d ' $now | Should -Be $now.AddDays(-1)
  }

  It 'reads a time, a time of day being today' {
    ConvertTo-LbtermSince '2026-10-20 18:30' | Should -Be ([datetime]'2026-10-20 18:30')
    ConvertTo-LbtermSince '18:30' | Should -Be ((Get-Date).Date.AddHours(18.5))
  }

  It 'turns down anything else' {
    { ConvertTo-LbtermSince 'yesterdayish' } | Should -Throw '*neither a time*'
  }
}

Describe 'ConvertTo-LbtermLogKey' {
  It 'takes out numbers, so pids and error codes read the same' {
    ConvertTo-LbtermLogKey 'pane 12 closed (os error 10054)' | Should -Be 'pane <n> closed (os error <n>)'
    ConvertTo-LbtermLogKey 'paint took 16.7ms' | Should -Be 'paint took <n>ms'
  }

  It 'takes out paths, however they were written' {
    ConvertTo-LbtermLogKey 'failed to set "C:\\Users\\Someone\\.local/share\\lbterm\\agent.5964" to "C:/x/y/agent.14184"' |
      Should -Be 'failed to set "<path>" to "<path>"'
    ConvertTo-LbtermLogKey 'connecting to C:/Code/LBTerm/target/try/sock.  Will try spawning the server.' |
      Should -Be 'connecting to <path>. Will try spawning the server.'
    ConvertTo-LbtermLogKey 'Running: "C:/a/perf/mux/lbterm-mux-server.exe" "--config-file" "C:/a/perf/mux/mux.lua"' |
      Should -Be (ConvertTo-LbtermLogKey 'Running: "C:/b/lbterm-mux-server.exe" "--config-file" "C:/b/split.lua"')
  }

  It "keeps a Rust source file's name, which says where a panic was" {
    ConvertTo-LbtermLogKey 'panic at C:\Users\Someone\.cargo\registry\src\wgpu-25.0.2\src\backend\wgpu_core.rs:330:5 - !?' |
      Should -Be 'panic at <path>\wgpu_core.rs:<n>:<n> - !?'
  }

  It 'takes out hex and GUIDs' {
    ConvertTo-LbtermLogKey 'GraphicsCommandList::close failed: The parameter is incorrect. (0x80070057)' |
      Should -Be 'GraphicsCommandList::close failed: The parameter is incorrect. (<hex>)'
    ConvertTo-LbtermLogKey 'this is 20261021-220928-003cb73b' | Should -Be 'this is <n>-<n>-<hex>'
    ConvertTo-LbtermLogKey 'session 0f8fad5b-d9cb-469f-a165-70867728950e gone' | Should -Be 'session <id> gone'
  }

  It "leaves words alone, and a message's lines run together" {
    ConvertTo-LbtermLogKey "No space for split!!! SplitDirectionAndSize {`n    direction: Horizontal,`n}" |
      Should -Be 'No space for split!!! SplitDirectionAndSize { direction: Horizontal, }'
    ConvertTo-LbtermLogKey 'added a decade' | Should -Be 'added a decade'
  }
}

Describe 'Group-LbtermLogProblems' {
  BeforeAll {
    function Log($Name, $Kind, [string[]]$Lines, [datetime]$Written) {
      $text = $Lines -join "`n"
      [pscustomobject]@{ Name = $Name; Path = "C:\logs\$Name"; Process = 'gui'; Pid = ($Name -replace '\D', ''); Kind = $Kind; Written = $Written
        Entries = @(ConvertFrom-LbtermLog $text $Written) }
    }
    $a = Log 'lbterm-gui.exe-log-1.txt' 'daily' @(
      (Line '10:00:00.000' 'ERROR' 'mux' 'lost client 5 (os error 10054)'),
      (Line '10:01:00.000' 'ERROR' 'mux' 'lost client 6 (os error 10054)'),
      (Line '10:02:00.000' 'INFO' 'mux' 'lost client 7 (os error 10054)'),
      (Line '10:03:00.000' 'WARN' 'lbterm::slow' 'slow paint window.rs:812: 41 ms'),
      (Line '10:04:00.000' 'WARN' 'wezterm_client::client' 'Running: "C:/a/lbterm-mux-server.exe"')) $day.AddHours(10).AddMinutes(5)
    $b = Log 'lbterm-gui.exe-log-2.txt' 'test' @(
      (Line '11:00:00.000' 'ERROR' 'mux' 'lost client 9 (os error 10053)'),
      (Line '11:30:00.000' 'WARN' 'wezterm_client::client' 'Running: "C:/b/lbterm-mux-server.exe"')) $day.AddHours(11).AddMinutes(31)
    $groups = @(Group-LbtermLogProblems @($a, $b))
  }

  It 'groups the same problem across logs, most often first' {
    $groups.Count | Should -Be 2
    $groups[0].Level | Should -Be 'ERROR'
    $groups[0].Target | Should -Be 'mux'
    $groups[0].Message | Should -Be 'lost client <n> (os error <n>)'
    $groups[0].Count | Should -Be 3
    $groups[0].First | Should -Be $day.AddHours(10)
    $groups[0].Last | Should -Be $day.AddHours(11)
    $groups[0].Logs.Name | Should -Be @('lbterm-gui.exe-log-1.txt', 'lbterm-gui.exe-log-2.txt')
    $groups[1].Message | Should -Be 'Running: "<path>"'
    $groups[1].Count | Should -Be 2
  }

  It 'leaves out INFO and the slow-frame lines' {
    $groups.Target | Should -Not -Contain 'lbterm::slow'
    ($groups | Measure-Object Count -Sum).Sum | Should -Be 5
  }

  It 'counts only what came since a time' {
    $since = @(Group-LbtermLogProblems @($a, $b) -Since $day.AddHours(10).AddMinutes(30))
    $since.Count | Should -Be 2
    $since[0].Count | Should -Be 1
    $since[0].Logs.Name | Should -Be @('lbterm-gui.exe-log-2.txt')
  }

  It 'shows each with its count, level, target, message, when and where' {
    $out = @(Format-LbtermLogProblems $groups)
    $out.Count | Should -Be 6
    $out[0] | Should -Match '^\s+3  ERROR  mux$'
    $out[1].Trim() | Should -Be 'lost client <n> (os error <n>)'
    $out[2].Trim() | Should -Be 'from 2026-10-21 10:00 to 2026-10-21 11:00 in 2 logs (daily 1, test 1): gui 2, gui 1'
  }
}

Describe 'ConvertFrom-LbtermSlowLine' {
  It 'reads one slow frame' {
    $s = ConvertFrom-LbtermSlowLine 'slow paint window.rs:812: 41.5 ms'
    $s.Kind | Should -Be 'paint'
    $s.Site | Should -Be 'window.rs:812'
    $s.Count | Should -Be 1
    $s.Ms | Should -Be 41.5
  }

  It 'reads many in the last 10 s' {
    $s = ConvertFrom-LbtermSlowLine 'slow task mux/src/localpane.rs:88: 12 times over 16 ms in the last 10 s, max 48 ms'
    $s.Kind | Should -Be 'task'
    $s.Site | Should -Be 'mux/src/localpane.rs:88'
    $s.Count | Should -Be 12
    $s.Ms | Should -Be 48
  }

  It 'reads a site that is not a place in a file' {
    $s = ConvertFrom-LbtermSlowLine 'slow message WM_PAINT: 20 ms'
    $s.Kind | Should -Be 'message'
    $s.Site | Should -Be 'WM_PAINT'
  }

  It 'says nothing of other messages' {
    ConvertFrom-LbtermSlowLine 'slow down' | Should -BeNullOrEmpty
    ConvertFrom-LbtermSlowLine 'slow scroll x: 20 ms' | Should -BeNullOrEmpty
    ConvertFrom-LbtermSlowLine '' | Should -BeNullOrEmpty
  }
}

Describe 'Group-LbtermSlowFrames' {
  BeforeAll {
    function Log($Name, $Kind, [string[]]$Lines, [datetime]$Written) {
      [pscustomobject]@{ Name = $Name; Path = "C:\logs\$Name"; Process = 'gui'; Pid = ($Name -replace '\D', ''); Kind = $Kind; Written = $Written
        Entries = @(ConvertFrom-LbtermLog ($Lines -join "`n") $Written) }
    }
    $a = Log 'lbterm-gui.exe-log-1.txt' 'daily' @(
      (Line '10:00:00.000' 'WARN' 'lbterm::slow' 'slow paint window.rs:812: 41 ms'),
      (Line '10:00:05.000' 'WARN' 'lbterm::slow' 'slow paint window.rs:812: 7 times over 16 ms in the last 10 s, max 90.5 ms'),
      (Line '10:00:06.000' 'WARN' 'lbterm::slow' 'slow task localpane.rs:88: 30 ms'),
      (Line '10:00:07.000' 'WARN' 'other' 'slow paint window.rs:812: 999 ms'),
      (Line '10:00:08.000' 'WARN' 'lbterm::slow' 'not a slow line')) $day.AddHours(10).AddMinutes(1)
    $b = Log 'lbterm-gui.exe-log-2.txt' 'test' @(
      (Line '11:00:00.000' 'WARN' 'lbterm::slow' 'slow paint window.rs:812: 20 ms')) $day.AddHours(11).AddMinutes(1)
    $groups = @(Group-LbtermSlowFrames @($a, $b))
  }

  It 'groups them by kind and site, counting the slow frames each line stands for' {
    $groups.Count | Should -Be 2
    $groups[0].Kind | Should -Be 'paint'
    $groups[0].Site | Should -Be 'window.rs:812'
    $groups[0].Count | Should -Be 9
    $groups[0].Lines | Should -Be 3
    $groups[0].Worst | Should -Be 90.5
    $groups[0].First | Should -Be $day.AddHours(10)
    $groups[0].Last | Should -Be $day.AddHours(11)
    $groups[0].Logs.Count | Should -Be 2
    $groups[1].Site | Should -Be 'localpane.rs:88'
  }

  It 'counts only what came since a time' {
    $since = @(Group-LbtermSlowFrames @($a, $b) -Since $day.AddHours(10).AddSeconds(30))
    $since.Count | Should -Be 1
    $since[0].Count | Should -Be 1
    $since[0].Worst | Should -Be 20
  }

  It 'shows each with its count, worst time, kind and site' {
    $out = @(Format-LbtermSlowFrames $groups)
    $out[0] | Should -Match '^\s+9  slow paint\s+worst\s+90\.5 ms  window\.rs:812$'
    $out[1].Trim() | Should -Be '3 lines from 2026-10-21 10:00 to 2026-10-21 11:00 in 2 logs (daily 1, test 1): gui 2, gui 1'
  }
}

Describe 'The views' {
  BeforeAll {
    function Log($Process, $Procid, $Kind, [datetime]$Written) {
      [pscustomobject]@{ Process = $Process; Pid = $Procid; Kind = $Kind; Alive = $false; Started = $Written.AddMinutes(-5)
        Written = $Written; Size = 2048; Errors = 3; Warnings = 1 }
    }
    $logs = @((Log 'gui' 5964 'daily' $day.AddHours(3)), (Log 'mux' 14184 'daily' $day.AddHours(2)), (Log 'cli' 220 'test' $day.AddHours(1)),
      (Log 'gui' 7 'unknown' $day), (Log 'gui' 8 'try' $day))
  }

  It 'counts the kinds in order' {
    Format-LbtermLogKinds $logs | Should -Be 'daily 2, try 1, test 1, unknown 1'
  }

  It 'names a few logs, newest first, and how many more' {
    Format-LbtermLogNames $logs -Max 3 | Should -Be 'gui 5964, mux 14184, cli 220 and 2 more'
    Format-LbtermLogNames $logs[0..1] | Should -Be 'gui 5964, mux 14184'
  }

  It 'gives sizes in B, KB and MB' {
    Format-LbtermLogSize 73 | Should -Be '73 B'
    Format-LbtermLogSize 23965 | Should -Be ('{0:0.#} KB' -f 23.4)
    Format-LbtermLogSize (3MB) | Should -Be '3 MB'
  }

  It 'lists one row a log under its columns' {
    $out = Format-LbtermLogList $logs[0..1]
    $rows = $out -split '\r?\n'
    $rows[0] | Should -Match '^kind\s+process\s+pid\s+alive\s+started\s+last written\s+size\s+errors\s+warnings$'
    $rows[2] | Should -Match '^daily\s+gui\s+5964\s+no\s+2026-10-21 02:55:00\s+2026-10-21 03:00:00\s+2 KB\s+3\s+1$'
    $rows.Count | Should -Be 4
  }
}

Describe 'lbterm.ps1 logs' {
  # The command itself, in a shell whose USERPROFILE is a folder of test logs
  BeforeAll {
    $script = Join-Path $PSScriptRoot '..\lbterm.ps1'
    $home2 = Join-Path $TestDrive 'home'
    $dir = Join-Path $home2 '.local\share\lbterm'
    $now = Get-Date
    $at = { param([datetime]$t) $t.ToString('HH:mm:ss.fff') }
    $t1 = $now.AddMinutes(-50)
    New-Log $dir 'lbterm-gui.exe-log-910001.txt' @(
      (Line (& $at $t1) 'INFO' 'lbterm_gui::conductor_hub' 'conductor hub: listening on \\.\pipe\conductor-hub'),
      (Line (& $at $t1.AddSeconds(1)) 'ERROR' 'wgpu_hal::auxil::dxgi::result' 'GraphicsCommandList::close failed: The parameter is incorrect. (0x80070057)'),
      (Line (& $at $t1.AddSeconds(2)) 'WARN' 'lbterm::slow' 'slow paint window.rs:812: 41 ms')) -Created $t1 -Written $t1.AddSeconds(2)
    $t2 = $now.AddMinutes(-20)
    New-Log $dir 'lbterm-gui.exe-log-910002.txt' @(
      (Line (& $at $t2) 'WARN' 'wezterm_client::client' 'Running: "C:/Code/LBTerm/target/release/lbterm-mux-server.exe" "--config-file" "C:/Code/LBTerm/target/perf/mux/mux.lua"'),
      (Line (& $at $t2.AddSeconds(1)) 'ERROR' 'wgpu_hal::auxil::dxgi::result' 'GraphicsCommandList::close failed: The parameter is incorrect. (0x887A0005)')) -Created $t2 -Written $t2.AddSeconds(1)

    function Invoke-Logs([string[]]$Arguments) {
      $saved = $env:USERPROFILE
      $env:USERPROFILE = $home2
      try { & 'C:\Program Files\PowerShell\7\pwsh.exe' -NoProfile -File $script logs @Arguments 2>&1 | Out-String }
      finally { $env:USERPROFILE = $saved }
    }
  }

  It 'lists the logs, newest first' {
    $out = Invoke-Logs @()
    $out | Should -Match '(?m)^test\s+gui\s+910002\s+no'
    $out | Should -Match '(?m)^daily\s+gui\s+910001\s+no'
    $out.IndexOf('910002') | Should -BeLessThan $out.IndexOf('910001')
  }

  It 'prints one log by its pid, and the newest of a kind' {
    $out = Invoke-Logs '-Pid', '910001'
    $out | Should -Match 'gui 910001: daily, gone \(listens on the conductor hub''s pipe'
    $out | Should -Match 'GraphicsCommandList::close failed'
    $out = Invoke-Logs '-Latest', '-Kind', 'test'
    $out | Should -Match 'gui 910002: test'
    $out | Should -Not -Match '910001'
  }

  It 'groups the errors of every kind, or of one' {
    $out = Invoke-Logs '-Errors'
    $out | Should -Match '(?m)^\s+2  ERROR  wgpu_hal::auxil::dxgi::result\r?$'
    $out | Should -Match 'in 2 logs \(daily 1, test 1\): gui 910002, gui 910001'
    $out | Should -Match 'Left out: 1 slow-frame lines'
    $out = Invoke-Logs '-Errors', '-Kind', 'daily'
    $out | Should -Match '(?m)^\s+1  ERROR  wgpu_hal'
  }

  It 'groups the slow frames, and narrows to a time' {
    Invoke-Logs '-Slow' | Should -Match '(?m)^\s+1  slow paint\s+worst\s+41 ms  window\.rs:812\r?$'
    Invoke-Logs '-Slow', '-Since', '30m' | Should -Match '(?m)^None\.\r?$'
  }

  It 'says so when there are none, and when a pid has no log' {
    Invoke-Logs '-Kind', 'try' | Should -Match 'No try logs in '
    Invoke-Logs '-Pid', '42' | Should -Match 'No log of pid 42'
  }
}
