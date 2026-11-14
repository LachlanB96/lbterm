#Requires -Version 7
<#
LBTerm's command line: build, check, install and start the fork, and see how far it is from upstream.

  pwsh -File lbterm\lbterm.ps1 build      release build of lbterm, lbterm-gui, lbterm-mux-server, strip-ansi-escapes
  pwsh -File lbterm\lbterm.ps1 check      THE CHECK: build, LBTerm's tests, then a smoke test of the built exes
  pwsh -File lbterm\lbterm.ps1 test -Package lbterm-extras   cargo test on the crates named, with the build's settings
  pwsh -File lbterm\lbterm.ps1 install    copy the build to %LOCALAPPDATA%\Programs\LBTerm, beside a running LBTerm
  pwsh -File lbterm\lbterm.ps1 rollback   swap back to the build the last install replaced, and restart into it
  pwsh -File lbterm\lbterm.ps1 start      open the installed LBTerm
  pwsh -File lbterm\lbterm.ps1 restart    end the GUI and open the installed one; the tabs live on in the mux server
  pwsh -File lbterm\lbterm.ps1 try        build, and open the build in a window of its own beside the daily LBTerm
  pwsh -File lbterm\lbterm.ps1 upstream   fetch wezterm/wezterm and count the commits on each side
  pwsh -File lbterm\lbterm.ps1 publish [-Push] [-Public <dir>] [-Message <text>]
                                          export main to the public repo's clone as one commit; -Push sends it
  pwsh -File lbterm\lbterm.ps1 logs       every LBTerm process's log, newest first: daily, try or test, errors, warnings
  pwsh -File lbterm\lbterm.ps1 logs -Pid 5964        print one log (-Latest: the newest), -Follow to follow it
  pwsh -File lbterm\lbterm.ps1 logs -Errors -Kind daily -Since 2h   ERROR and WARN lines grouped by kind
  pwsh -File lbterm\lbterm.ps1 logs -Slow     the slow-frame lines (lbterm::slow) grouped by kind and site
  pwsh -File lbterm\lbterm.ps1 logs -Budget   the budget's lines (lbterm::budget): what ran at rest by who asked, actions over their time
  pwsh -File lbterm\lbterm.ps1 e2e -Only a,b [-Bin <dir>] [-Retry 1] [-WaitIdle]
                                          the end-to-end tests in lbterm\perf named, one at a time (not part of the check)
  pwsh -File lbterm\lbterm.ps1 e2e -Smoke [-Only a,b]   a branch's last stage: the smoke set (lbterm\perf\smoke.json)
                                          and the scripts the branch added or changed, about 5 minutes and theirs
  pwsh -File lbterm\lbterm.ps1 e2e -Changed   the scripts whose Covers lines match what changed: candidates, nothing run
  pwsh -File lbterm\lbterm.ps1 e2e -Only a -TestLog 'info,lbterm::present::stats=trace'   a trace in every test window
  pwsh -File lbterm\lbterm.ps1 e2e -AllScripts   every script: a whole set, only when Lachlan asks for one
  pwsh -File lbterm\lbterm.ps1 budgets -Only a,b|rest|action [-Bin <dir>] [-Runs n] [-Seconds s]
                                          what each scenario of lbterm\perf\budgets.json costs at rest, and how
                                          long each action takes and costs, held to its budgets
  pwsh -File lbterm\lbterm.ps1 budgets -Against <dir> [-Rounds 5] [-Only a,b] [-Bin <dir>]
                                          the same, -Bin's build against the build in <dir>, the two taken in
                                          turn, each difference held to how much each build differs from itself
  pwsh -File lbterm\lbterm.ps1 look -Only a,b -Against <dir> [-Bin <dir>]
                                          a look judged by eye: each script on the build in <dir> and on -Bin's,
                                          every capture both saved side by side, for show-image
  pwsh -File lbterm\lbterm.ps1 slot -Branch feat/x   start a branch in a build slot, a worktree whose build is kept
  pwsh -File lbterm\lbterm.ps1 slot -Free slot-2     put a slot back on main without landing its branch
  pwsh -File lbterm\lbterm.ps1 slots      the slots, and the branch each has out
  pwsh -File lbterm\lbterm.ps1 prune      what builds and e2e runs left in every tree (the main checkout, slots,
                                          worktrees), as each build and e2e run prunes its own tree
  pwsh -File lbterm\lbterm.ps1 unify      write lbterm-unify\Cargo.toml, so every cargo run agrees on features

slot: a worktree built from nothing takes 11 to 12 minutes, so a branch starts in one of six
slots (.claude\worktrees\slot-1 to slot-6) instead, which land never removes: it moves the slot to
main with nothing out and keeps its build (land keeps a worktree locked "slot: ..."),
so the next branch there rebuilds only what changed. It takes the free slot closest to main, makes
one while there are fewer than six, and when all six are busy gives the branch a worktree of
its own as before. Not while under 20 GB of disk is free.

prune: nothing in cargo prunes a target folder, so a slot grew past 20 GB and C: fell under the
warden's 20 GB floor (2026-11-04, 2026-11-08). Each build deletes the incremental folders earlier
builds left in its tree (a crate's newest stays, and any written in the last 2 days), each e2e run
the runs before it older than 3 days (the newest stays), and prune does both in every tree:
lbterm-lib.ps1, "What builds and test runs leave behind".

unify: cargo works out each library's features from the packages a command names, and the check's
cargo test names others than the build (and brings in upstream's test libraries), so it compiled
about 150 crates a second time. Every cargo run here also names lbterm-unify, which depends on each
library that would differ, with all its features; unify writes it, and the check fails when an
upstream merge or a new dependency makes them differ again (lbterm-lib.ps1, "One set of features").

The check runs lbterm\tests' Pester beside the build and prints its output after cargo's.

logs reads ~\.local\share\lbterm, where every lbterm-gui, lbterm-mux-server and lbterm (cli) process
writes <exe>-log-<pid>.txt, from the daily LBTerm, try windows and test windows alike. Each log's
kind: a running process's is where its exe is (the install folder is daily, a tree's target\try\bin
try, anywhere else test); a finished one's is what its log names (a try build's socket or server, a
tree's target or the scratchpad, LBTERM_TEST, debug logging, which only tests turn on; else the
install folder, the daily mux server's socket, the conductor hub's pipe), or unknown when it names
none of them. -Kind daily|try|test|unknown and -Since (18:00, 2026-10-21 18:00, 30m, 2h, 1d) narrow
every view. -Errors takes out what changes from one line to the next (numbers, pids, hex, paths
but a Rust source file's name) so the same problem groups together. The lines carry times of day
only: they're dated back from when each log was last written.

e2e runs the test scripts -Only names (a script's name runs its Variant lines too) against -Bin
(this tree's target\release by default), with its output in target\e2e\<run>\<script>; a failure
runs again -Retry times and is flaky if a rerun passes. report.json and a table at the end; exit 1
when one fails. Never a whole set unless -AllScripts says so (Lachlan's 10a and 11a of 2026-11-09: whole
sets of 80 to 150 scripts took two to three hours, read as a fail on one script that fails on
main's build too, and told nothing a named run wouldn't). A branch's last stage runs -Smoke: the
smoke set, the dozen runs lbterm\perf\smoke.json names (losing tabs, key echo, reading Claude Code,
a window taking focus, a lost GPU), and every script the branch added or changed, with its variants
(Lachlan's 1a of 2026-11-09: of 529 failed attempts in the history, 6 were a branch's where main's
build passed the same script that day; .claude\tools\e2e-history.ts). -Changed lists the scripts whose Covers
globs match a file changed since main (committed or not) and runs none, so one that judges what
changed and nothing in -Smoke does can be added by name. A script that fails is run again by name, and judged
against main's build run on that script alone. e2e, budgets and look ask Windows to keep the
displays on while they run (harness.psm1's Request-DisplaysAwake, Lachlan's E4 of 2026-11-10),
turn them back on with a mouse move of no pixels if they're off (his 2a), and refuse to start, or an e2e run stops, when Windows says they're off (Get-DisplaysAsleep): DWM then
plays no motion and blocks the draw step, so most scripts fail on every build (PITFALLS 239).

budgets runs each scenario of lbterm\perf\budgets.json (-Only some, by name or wildcard, or a kind's
name, rest or action, for all of that kind) in a test window of -Bin's build (this tree's
target\release by default) through lbterm\perf\budgets.ps1, one process each. One at rest is
measured -Runs spans (budgets.json's 3 by default) of -Seconds each, every metric's median held to
its budget; an action is done its runs (15 after a warm-up, unless budgets.json or -Runs says),
each timed from what starts it to the first frame that shows it and costed to the window quiet
again, the median and the 95th percentile held to theirs (lbterm/PERF.md, "The budget runner").
Output in target\budgets\<run>\<scenario>, report.json and a table at the end, and one line a run
appended to target\budgets\history.jsonl, so a drift shows from build to build; exit 1 when one is
over budget or couldn't be measured. Not part of the check: some fail until what they found is
fixed. Every span and run also says what the rest of the PC did meanwhile (what others took of the
CPUs it ran on, how busy the PC was, the clock): one on a busy PC (budgets.json's pc rules) is taken
again, CPU measured by cycles is scaled to the nominal clock, and a scenario over budget with too few
calm spans or runs is busy, not a failure (lbterm/PERF.md, "The PC while it measured").

look (Lachlan's 3b of 2026-11-09): how something looks or moves is his to judge, so a change to a
look is shown to him once rather than held to a pixel threshold on every branch after it. Each
script -Only names runs once on the build in -Against (before) and once on -Bin's (after), and each
capture both runs saved becomes one picture, the two side by side with their builds named over them,
in target\look\<run>; the line it ends with runs show-image on them. The scripts' own checks are
said and judge nothing. A test-window run, so inside a warden job, and not on a locked PC or while
the displays sleep, whose captures show nothing of DirectComposition.

budgets -Against <dir> holds -Bin's build against the build in <dir> (a copy of the installed one,
for a branch's before and after): each scenario -Rounds times on each (5 by default), the order
turned each round, and each metric's difference held to how each build's rounds vary (Welch's test
on the rounds' values at 99%): higher or lower once its interval leaves out nothing, else same.
More rounds tell smaller differences apart. report.json and a table; exit 1 as above, by -Bin's
results.

A running LBTerm keeps its files open, so install renames each one aside (<name>.<stamp>.old) and copies
the new one in; the next install deletes the old ones nothing holds any more. With the mux domain on
(lbterm/MUX.md) the tabs live in lbterm-mux-server, so restart swaps the GUI without ending them.

Install writes the build's version (wezterm.version in Lua) to version.txt beside the exes, and
keeps the build it replaces whole in %LOCALAPPDATA%\Programs\LBTerm-previous. rollback swaps the
two, so a second rollback goes forward again.

This PC's CPU must never pass 94 C, so build, check, test, unify and try are started as Process
Warden jobs by their caller (warden-run: pinned CPUs, below-normal priority, a memory cap, its heat
steps and its THERMAL-STOP). This script starts no job of its own: cargo runs straight inside the
job it's in, with -j 4, and outside one nothing holds its rustc to that. A PC that asks for it (a
"job" setting in ~\.config\lbterm\dev.json: lbterm-lib.ps1, "Cargo only inside a job") has these
refused outside a job, before anything runs, with the line that starts them in one, and e2e and
budgets too: their test windows open outside the job, but the warden queues the run while the PC
is hot and counts its CPUs. Nothing here runs a guard of its own: another project's
guarded.sh, which cargo ran under until 2026-10-25, suspended compilers PC-wide and stopped every
agent's jobs, and must never run inside a warden job.

publish makes the public repo's next commit in its clone (-Public, else LBTerm-public beside the
main checkout): main's tree without .claude\ and CLAUDE.md, on top of the last export, with
WezTerm's history under it and never the private repo's (Publish-Lbterm says how). The clone's
hooks run lbterm\publish\leak-check.sh on the commit, its message, and every push. Its message
lists the merges into main since the last export unless -Message gives one. -Push pushes the
clone's main to its origin.

Release builds here are incremental and linked by rust-lld, and OpenSSL is built once per version
and kept (Invoke-Cargo says why). When sccache is installed, cargo runs through it (RUSTC_WRAPPER)
and its server is started outside Claude's job (it only reads and writes the cache: SCCACHE_CLIENT_SIDE has
every compile run in its own client under cargo, inside the warden's job and on its CPUs); -NoSccache
turns it off for a run (Use-Sccache).
Run cargo through this script: a cargo run without the same settings builds a second set of
everything beside this one.
#>
param(
  [Parameter(Position = 0)]
  [ValidateSet('build', 'check', 'test', 'install', 'rollback', 'start', 'restart', 'try', 'upstream', 'publish', 'logs', 'e2e', 'budgets', 'look', 'slot', 'slots', 'prune', 'unify')]
  [string]$Command = 'build',
  # test: the crates whose tests run
  [string[]]$Package,
  # test: only their ignored tests (cargo test -- --ignored): the reports that need an input of
  # your own, such as lbterm-extras' report_the_context_bar_on_an_own_transcript (CONTEXT.md)
  [switch]$Ignored,
  # slot: the branch to start (or carry on) in a slot
  [string]$Branch,
  # slot: the slot to put back on main without landing its branch
  [string]$Free,
  # cargo without sccache, though it's installed
  [switch]$NoSccache,
  [int]$Jobs = 4,
  # logs: one process's log ($PID is PowerShell's own, so the variable has another name)
  [Alias('Pid')][int]$ProcessId,
  # logs: the newest log, printed; with -Follow, followed as it's written
  [switch]$Latest,
  [switch]$Follow,
  # logs: ERROR and WARN lines grouped by kind; the slow-frame lines grouped by kind and site; the
  # budget's lines, what ran at rest and actions over their time (lbterm/PERF.md)
  [switch]$Errors,
  [switch]$Slow,
  [switch]$Budget,
  # logs: only the daily LBTerm's, try windows', test windows', or those that say nothing of which
  [ValidateSet('daily', 'try', 'test', 'unknown')][string]$Kind,
  # logs: only what was written since then: 18:00, 2026-10-21 18:00, 30m, 2h, 1d
  [string]$Since,
  # e2e: the scripts to run (names, or a script's name for it and its variants), else all;
  # budgets: the scenarios to run, by name or wildcard, or rest or action for every one of a kind
  [string[]]$Only,
  # e2e: list the scripts whose Covers lines match what changed since main, running none
  [switch]$Changed,
  # e2e: the smoke set (lbterm\perf\smoke.json) and the scripts the branch added or changed, beside
  # any -Only names
  [switch]$Smoke,
  # e2e: WEZTERM_LOG for every test window the scripts open (module=level), through LBTERM_E2E_LOG,
  # since a window never takes WEZTERM_ variables from the run's environment. Not -Log: logs keeps
  # its log in $log, the same variable (PowerShell's names ignore case, PITFALLS 245)
  [string]$TestLog,
  # e2e, budgets: every script or scenario, a whole set: only when Lachlan asks for one. Not -All:
  # `logs` keeps its logs in $all, the same variable (PowerShell's names ignore case, PITFALLS 245)
  [switch]$AllScripts,
  # e2e, budgets: the build the test windows run, else target\release
  [string]$Bin,
  # e2e: how many times a failure runs again (budgets: a scenario that couldn't be measured)
  [int]$Retry = 1,
  # budgets: how many spans each scenario at rest is measured for (and each action done), else
  # budgets.json's; and how long each span is
  [int]$Runs,
  [double]$Seconds,
  # e2e: how long one script may run before it's stopped
  [int]$TimeoutMin = 10,
  # e2e: each test window waits until the keyboard and mouse have been idle for 3 s
  [switch]$WaitIdle,
  # budgets: hold -Bin's build against the build in this folder, so many rounds of each
  [string]$Against,
  [int]$Rounds = 5,
  # publish: the public repo's clone, else LBTerm-public beside the main checkout
  [string]$Public,
  # publish: the export's commit message, else the merges since the last one
  [string]$Message,
  # publish: push the clone's main to its origin
  [switch]$Push
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'lbterm-lib.ps1')

$repo    = Split-Path $PSScriptRoot -Parent
$release = Join-Path $repo 'target\release'
# The main checkout, which a worktree shares its OpenSSL with
$main    = [IO.Path]::GetFullPath((Split-Path (git -C $repo rev-parse --path-format=absolute --git-common-dir) -Parent))
$install = Join-Path $env:LOCALAPPDATA 'Programs\LBTerm'
# The build the last install replaced, which rollback swaps back in
$previous = Join-Path $env:LOCALAPPDATA 'Programs\LBTerm-previous'
$pwsh    = 'C:\Program Files\PowerShell\7\pwsh.exe'
# The vendored OpenSSL build needs a Windows perl. Git's MSYS perl writes paths nmake can't read.
$perl    = 'C:\Strawberry\perl\bin'

$exes   = $LbtermExes
$extras = $LbtermExtras
# The mux domain's socket (lbterm/MUX.md): lbterm.lua leaves socket_path at its default
$muxSock = Join-Path $env:USERPROFILE '.local\share\lbterm\sock'

function Get-Installed([string[]]$names) {
  @(Get-Process $names -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$install\*" })
}

# A GUI that can't attach to the running server would leave its tabs out of reach. The codec
# version only changes with an upstream merge that changes the protocol; $cli is the lbterm.exe
# of the build about to be put in.
function Assert-ServerReachable([string]$cli, [string]$what) {
  $server = Get-Installed lbterm-mux-server
  if (-not $server) { return }
  $env:WEZTERM_UNIX_SOCKET = $muxSock
  $out = & $cli cli --no-auto-start list 2>&1 | Out-String
  if ($out -match 'codec version') {
    throw "The running lbterm-mux-server (pid $($server.Id -join ', ')) speaks another protocol than $what, so its GUI couldn't reach the tabs. Close LBTerm, which ends them, then try again.`n$out"
  }
}

# Ends the GUI and opens the installed one. Killing the GUI is what a crash does: panes in the mux
# server live on, and the new GUI attaches to them. Local panes would die with it, so only with a
# server running.
function Restart-Gui {
  $gui = Join-Path $install 'lbterm-gui.exe'
  if (-not (Test-Path $gui)) { throw "LBTerm isn't installed: run install first" }
  if (-not (Get-Installed lbterm-mux-server)) {
    throw "No lbterm-mux-server is running, so LBTerm's tabs live in the GUI and would end with it. Close and open LBTerm yourself."
  }
  $cli = Join-Path $install 'lbterm.exe'
  $env:WEZTERM_UNIX_SOCKET = $muxSock
  $inServer = @(& $cli cli --no-auto-start list --format json | ConvertFrom-Json).Count
  foreach ($p in Get-Installed lbterm-gui) {
    # A GUI started without the mux domain has panes of its own, and lists more than the server
    $env:WEZTERM_UNIX_SOCKET = Join-Path $env:USERPROFILE ".local\share\lbterm\gui-sock-$($p.Id)"
    $inGui = @(& $cli cli --no-auto-start list --format json | ConvertFrom-Json).Count
    if ($LASTEXITCODE -or $inGui -gt $inServer) {
      throw "LBTerm (pid $($p.Id)) has tabs of its own, outside the mux server, which would end with it. Close and open it yourself."
    }
  }
  foreach ($p in Get-Installed lbterm-gui) { Stop-Process -Id $p.Id -Force; $p.WaitForExit(5000) | Out-Null }
  explorer.exe $gui
  'restarted LBTerm: its tabs come back from the mux server'
}

# sccache's server, running before cargo starts. Started here, outside Claude's job, with
# Start-Unfocused (WMI, no console): a server cargo's first client starts is that client's child,
# so inside the warden's job, and it dies with the job while other builds still use it.
function Start-SccacheServer([string]$Exe) {
  $port = if ($env:SCCACHE_SERVER_PORT -match '^\d+$') { [int]$env:SCCACHE_SERVER_PORT } else { 4226 }
  if (Test-SccacheServer $port) { return "sccache: using its server on port $port" }
  # The client's settings stay out of the server's environment, which Start-Unfocused copies
  foreach ($k in @((Get-SccacheEnvironment).Keys) + 'RUSTC_WRAPPER') { Remove-Item "Env:$k" -ErrorAction SilentlyContinue }
  . (Join-Path $PSScriptRoot 'motion\start-unfocused.ps1')
  $id = Start-Unfocused -Exe $Exe -NoConsole -WorkingDirectory (Split-Path $Exe) -Environment ([hashtable](Get-SccacheEnvironment -Server))
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while (-not (Test-SccacheServer $port) -and $clock.Elapsed.TotalSeconds -lt 15) { Start-Sleep -Milliseconds 200 }
  $alive = [bool](Get-Process -Id $id -ErrorAction SilentlyContinue)
  $listening = Test-SccacheServer $port
  if ($alive -and $listening) {
    "sccache: started its server outside this job (pid $id, port $port)"
  } elseif ($listening) {
    # Another build's client started one in between, so this one found the port taken
    "sccache: its server (pid $id) exited; using the one another build started on port $port"
  } else {
    "sccache: its server (pid $id) isn't listening on port $port after 15 s; cargo's first client will start one inside this job"
  }
}

# sccache, when it's installed (Find-Sccache) and -NoSccache isn't given: cargo runs rustc through it
# (RUSTC_WRAPPER), which caches the registry's crates. LBTerm's own crates build incrementally,
# which it can't cache, so it hands them straight to rustc; bin and proc-macro crates (build scripts
# too) link, so neither can those. Cargo's fingerprint doesn't include RUSTC_WRAPPER, and the build
# scripts that read it don't rerun for it, so switching sccache on or off rebuilds nothing.
#
# C isn't cached. cc-rs hands a compile to RUSTC_WRAPPER only when CC or CXX names the compiler,
# which also needs vcvars64's INCLUDE and LIB, and cc-rs reruns its build scripts when those change:
# 53 crates and 5.5 minutes once in a tree built without them. Tried on 2026-10-22: sccache's
# preprocessor cache works for GCC and Clang only, so an MSVC hit still runs cl /E, and with
# windows.h in most of the sources a fresh worktree's C took as long from the cache (613 s across
# the crates) as compiled (567 s). Only harfbuzz, one large C++ unit, gained (27 s to 1 s).
function Use-Sccache {
  Remove-Item Env:RUSTC_WRAPPER -ErrorAction SilentlyContinue
  if ($NoSccache) { return }
  $sccache = Find-Sccache
  if (-not $sccache) { return }
  Start-SccacheServer $sccache
  foreach ($e in (Get-SccacheEnvironment).GetEnumerator()) { Set-Item "Env:$($e.Key)" $e.Value }
  $env:RUSTC_WRAPPER = $sccache
}

function Invoke-Cargo([string[]]$cargoArgs) {
  if (-not (Test-Path "$perl\perl.exe")) { throw "No Strawberry Perl at $perl (winget install StrawberryPerl.StrawberryPerl)" }
  $env:PATH = "$perl;$env:PATH"
  # Git's bash can put its MSYS perl back in front on PATH (it did when lbterm.ps1 was started
  # from a Git Bash shell). openssl-src takes this over PATH.
  $env:OPENSSL_SRC_PERL = "$perl\perl.exe"
  # Incremental release builds, linked by LLVM's lld (it comes with rustc). Measured 2026-10-20: a
  # one-line change to wezterm-gui went from 2 m 06 s to 30 s, one to term (which rebuilds the ~30
  # crates above it) from about 5.5 minutes to 34 s, and cli-list.ps1 timed the result the same.
  # Through the release profile, not CARGO_INCREMENTAL: sccache (RUSTC_WRAPPER) refuses to run at
  # all while CARGO_INCREMENTAL is set. Cargo's profile, so its fingerprint, is the same either way;
  # sccache hands LBTerm's own crates (built incrementally) straight to rustc and caches the
  # registry's, which cargo never builds incrementally.
  Remove-Item Env:CARGO_INCREMENTAL, Env:CARGO_BUILD_INCREMENTAL -ErrorAction SilentlyContinue
  $env:CARGO_PROFILE_RELEASE_INCREMENTAL = 'true'
  $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = 'rust-lld.exe'
  # The incremental folders earlier builds left in this tree, which nothing else prunes, so a
  # slot stays at about its last build's size (Remove-StaleBuildFiles: a crate's newest folder stays)
  Format-StaleBuildFiles (Remove-StaleBuildFiles -Tree $repo -What incremental) 'incremental'
  Use-Sccache
  # OpenSSL, once per version. openssl-src's own build (one nmake job) was 12.5 of the 17 minutes
  # a build from nothing took, and every worktree repeated it. The first build of a version keeps
  # what it built in the main checkout's target\openssl\<version>; builds after link that instead,
  # since OPENSSL_NO_VENDOR wins over the openssl crate's vendored feature.
  $sslVersion = Get-OpensslSrcVersion (Join-Path $repo 'Cargo.lock')
  $sslCache = Join-Path $main "target\openssl\$sslVersion"
  $builds = Join-Path $repo 'target\release\build'
  $cached = $sslVersion -and (Save-OpensslCache -BuildDir $builds -Cache $sslCache -SrcVersion $sslVersion)
  if ($cached) {
    $env:OPENSSL_NO_VENDOR = '1'
    $env:OPENSSL_DIR = $sslCache
    $env:OPENSSL_STATIC = '1'
  }
  Push-Location $repo   # cargo reads .cargo/config.toml from the working directory
  try {
    # Straight inside the warden job this runs in: the job is the thermal guard (lbterm.ps1's help)
    & cargo @cargoArgs
    if ($LASTEXITCODE) { throw "cargo $($cargoArgs -join ' ') failed (exit $LASTEXITCODE)" }
  } finally { Pop-Location }
  if ($sslVersion -and -not $cached -and (Save-OpensslCache -BuildDir $builds -Cache $sslCache -SrcVersion $sslVersion)) {
    "kept OpenSSL $sslVersion for the builds after this one: $sslCache"
  }
}

# stdout of an exe, read through a real pipe. PowerShell's own & neither captures nor waits for a
# GUI-subsystem exe like lbterm-gui.
function Get-Output([string]$exe, [string]$arg) {
  $psi = [Diagnostics.ProcessStartInfo]::new($exe, $arg)
  $psi.RedirectStandardOutput = $true
  $psi.UseShellExecute = $false
  $p = [Diagnostics.Process]::Start($psi)
  $out = $p.StandardOutput.ReadToEnd()
  $p.WaitForExit()
  $out.Trim()
}

# -p for each of $Packages, and lbterm-unify, which every cargo run here names
function Get-PackageArgs([string[]]$Packages) { @($Packages + $LbtermUnify | Select-Object -Unique | ForEach-Object { '-p', $_ }) }

function Build {
  Invoke-Cargo (@('build', '--release', '-j', $Jobs) + (Get-PackageArgs $LbtermBuildPackages))
}

# What one cargo run would resolve for $Packages (with lbterm-unify): Split-CargoFeatures of its
# two cargo trees, the target side's and the whole. -Dev for a cargo test, whose dev-dependencies
# count. cargo tree compiles nothing.
function Get-CargoFeatures([string[]]$Packages, [switch]$Dev) {
  $tree = @('tree', '--manifest-path', (Join-Path $repo 'Cargo.toml'), '--target', 'x86_64-pc-windows-msvc',
    '--prefix', 'none', '-f', '{p}|{f}') + (Get-PackageArgs $Packages)
  # (PowerShell's names ignore case, so not $dev, which is -Dev)
  $devEdge = if ($Dev) { ',dev' } else { '' }
  $out = @{}
  Push-Location $repo   # cargo reads .cargo/config.toml from the working directory
  try {
    foreach ($side in @{ Name = 'target'; Edges = "normal$devEdge,no-proc-macro" }, @{ Name = 'full'; Edges = "normal,build$devEdge" }) {
      $lines = @(& cargo @tree -e $side.Edges 2>&1 | ForEach-Object { "$_" })
      if ($LASTEXITCODE) { throw "cargo tree for $($Packages -join ', ') failed (exit $LASTEXITCODE): $(($lines | Where-Object { $_ -notmatch '\|' }) -join ' ')" }
      $out[$side.Name] = ConvertFrom-CargoTree $lines
    }
  } finally { Pop-Location }
  Split-CargoFeatures $out.target $out.full
}

# The cargo runs that must agree: the build's, the check's cargo test, and with -All each crate's
# test on its own (lbterm.ps1 test -Package)
function Get-UnifySelections([switch]$All) {
  $s = [ordered]@{
    build = @{ Packages = $LbtermBuildPackages; Dev = $false }
    check = @{ Packages = $LbtermTestPackages; Dev = $true }
  }
  if ($All) { foreach ($p in $LbtermTestPackages) { $s["test $p"] = @{ Packages = @($p); Dev = $true } } }
  $s
}

function Get-UnifyDiff([switch]$All) {
  $res = [ordered]@{}
  foreach ($e in (Get-UnifySelections -All:$All).GetEnumerator()) { $res[$e.Key] = Get-CargoFeatures $e.Value.Packages -Dev:$e.Value.Dev }
  @(Compare-CargoFeatures $res)
}

# Writes lbterm-unify\Cargo.toml from nothing: every library two selections give different
# features, with all of them, in rounds until none differ (a pin's features can turn on optional
# dependencies, which can differ in turn)
function Update-Unify {
  $dir = Join-Path $repo $LbtermUnify
  $manifest = Join-Path $dir 'Cargo.toml'
  $pins = [ordered]@{}
  [IO.File]::WriteAllText($manifest, (New-UnifyManifest -Pins @() -Dir $dir))
  $diff = @()
  for ($round = 1; $round -le 6; $round++) {
    $diff = Get-UnifyDiff -All
    "round ${round}: $($diff.Count) $(if ($diff.Count -eq 1) { 'library differs' } else { 'libraries differ' })$(if ($diff) { ': ' + (($diff | ForEach-Object { "$($_.Name) $($_.Version)$(if ($_.Side -eq 'Host') { ' (host)' })" }) -join ', ') })"
    if (-not $diff) { break }
    foreach ($d in $diff) {
      $k = "$($d.Side) $($d.Key)"
      if ($pins.Contains($k)) {
        $set = [Collections.Generic.SortedSet[string]]::new([string[]]($pins[$k].Features + $d.Features), [StringComparer]::Ordinal)
        $d.Features = @($set)
      }
      $pins[$k] = $d
    }
    [IO.File]::WriteAllText($manifest, (New-UnifyManifest -Pins @($pins.Values) -Dir $dir))
  }
  if ($diff) { throw "after 6 rounds $($diff.Count) libraries still differ: $(($diff | ForEach-Object Key) -join ', ')" }
  "wrote $manifest with $($pins.Count) $(if ($pins.Count -eq 1) { 'library' } else { 'libraries' }): commit it with Cargo.lock"
  Remove-Item -LiteralPath (Join-Path $repo 'target\lbterm-unify.ok') -ErrorAction SilentlyContinue
}

# The check's test that the build and its cargo test still agree, so cargo test compiles only the
# test harnesses. The comparison (four cargo trees, about 10 s) runs again only when a Cargo.toml,
# Cargo.lock, the selections or cargo change.
function Assert-Unified {
  $ok = Join-Path $repo 'target\lbterm-unify.ok'
  $key = Get-UnifyKey -Repo $repo -Cargo "$(& cargo -V)"
  if ((Test-Path -LiteralPath $ok) -and [IO.File]::ReadAllText($ok).Trim() -eq $key) {
    return "features: the build and the tests agree (no Cargo.toml or Cargo.lock changed since they were compared)"
  }
  $diff = Get-UnifyDiff
  if ($diff) {
    throw "The build and the check's cargo test would build $($diff.Count) $(if ($diff.Count -eq 1) { 'library' } else { 'libraries' }) with different features ($(($diff | ForEach-Object { "$($_.Name) $($_.Version), missing in $($_.Missing -join ' and ')" }) -join '; ')), and cargo test would compile them and everything above them a second time. Run lbterm.ps1 unify, then commit lbterm-unify\Cargo.toml and Cargo.lock."
  }
  New-Item -ItemType Directory -Force (Split-Path $ok) | Out-Null
  # the key again: cargo tree can have written Cargo.lock
  [IO.File]::WriteAllText($ok, (Get-UnifyKey -Repo $repo -Cargo "$(& cargo -V)"))
  'features: the build and the tests agree'
}

# Where every LBTerm process writes its log (config's RUNTIME_DIR)
$logDir = Join-Path $env:USERPROFILE '.local\share\lbterm'

# The LBTerm processes running now, by pid, as Get-LbtermLogs takes them
function Get-LiveLbterm {
  $live = @{}
  foreach ($p in Get-Process lbterm, lbterm-gui, lbterm-mux-server -ErrorAction SilentlyContinue) {
    $started = try { $p.StartTime } catch { [datetime]::MinValue }
    $live[$p.Id] = @{ Name = $p.ProcessName; Path = $p.Path; StartTime = $started }
  }
  $live
}

function Get-LogHeader($log) {
  $state = if ($log.Alive) { 'running' } else { 'gone' }
  "$($log.Process) $($log.Pid): $($log.Kind), $state ($($log.Reason)); started $($log.Started.ToString('yyyy-MM-dd HH:mm:ss')), last written $($log.Written.ToString('yyyy-MM-dd HH:mm:ss')), $(Format-LbtermLogSize $log.Size), $($log.Errors) errors, $($log.Warnings) warnings"
  $log.Path
}

# One log, its lines since $From
function Show-Log($log, $From) {
  Get-LogHeader $log
  ''
  foreach ($e in $log.Entries) { if ($null -eq $From -or $e.Time -ge $From) { $e.Lines } }
}

# Follows a log from its last 20 lines as its process writes it, until the process exits (or Ctrl+C)
function Watch-Log($log) {
  Get-LogHeader $log
  ''
  $stream = [IO.File]::Open($log.Path, 'Open', 'Read', [IO.FileShare]'ReadWrite, Delete')
  try {
    $reader = [IO.StreamReader]::new($stream)
    @($reader.ReadToEnd() -split '\r?\n' | Where-Object { $_ }) | Select-Object -Last 20
    # Found by name: an install renames a running exe aside, and Get-Process -Id then names the
    # process after the renamed file (lbterm-mux-server.exe.20261020220512.old)
    $proc = if ($log.Alive) { Get-Process -Name $log.Exe -ErrorAction SilentlyContinue | Where-Object Id -eq $log.Pid }
    if (-not $proc) { "-- $($log.Process) $($log.Pid) has gone: nothing more will come"; return }
    "-- following $($log.Process) $($log.Pid) until it exits; Ctrl+C stops"
    $partial = ''
    while ($true) {
      $chunk = $reader.ReadToEnd()
      if ($chunk) {
        # Only whole lines: the rest waits for its newline
        $parts = ($partial + $chunk) -split '\r?\n'
        $partial = $parts[-1]
        $parts | Select-Object -SkipLast 1
        continue
      }
      if ($proc.HasExited) {
        if ($partial) { $partial }
        "-- $($log.Process) $($log.Pid) has exited"
        break
      }
      Start-Sleep -Milliseconds 250
    }
  } finally { $stream.Dispose() }
}

# Cargo only inside a job, on a PC that asks for it (lbterm-lib.ps1, "Cargo only inside a job"),
# and the test-window runs too (e2e, budgets: Lachlan, 2026-11-01, after runs outside the warden
# heated the PC): refused here, before anything runs, with the line that starts it in one
if ($Command -in 'build', 'check', 'test', 'unify', 'try', 'e2e', 'budgets', 'look') {
  $dev = Get-DevSettings
  $refusal = if ($dev.job.env) {
    Get-JobRefusal -Settings $dev -Command $Command -Script $PSCommandPath `
      -Arguments (Format-LbtermArguments $PSBoundParameters) `
      -Value ([Environment]::GetEnvironmentVariable([string]$dev.job.env)) -InJob (Test-InJobObject)
  }
  if ($refusal) {
    [Console]::Error.WriteLine($refusal)
    exit 2
  }
}

switch ($Command) {
  'build' { Build }

  'check' {
    # lbterm\tests' Pester (lbterm-lib.ps1, which this script's own file handling is), in a fresh
    # shell so nothing loaded here leaks in, beside the build, which it needs none of, and takes
    # about 100 s. All but try.Tests.ps1, which runs this tree's target\release\lbterm.exe: beside
    # cargo it caught the exes being linked again ("The system cannot find the file specified",
    # 2026-10-29), so it runs once the build and cargo test are done
    $tests = Join-Path $PSScriptRoot 'tests'
    $afterBuild = Join-Path $tests 'try.Tests.ps1'
    $pester = Start-PesterRun -Tests $tests -Exclude $afterBuild -Log (Join-Path $repo 'target\check\pester.log') -Pwsh $pwsh
    "Pester: lbterm\tests running beside cargo; its output follows cargo's"
    try {
      Assert-Unified
      Build
      # A failure already in means no tests need building
      if ($pester.Process.HasExited -and $pester.Process.ExitCode) {
        Wait-PesterRun $pester | Out-Null
        throw "Pester: lbterm\tests failed"
      }
      # The crates LbtermTestPackages names (lbterm-lib.ps1 says what each one's tests cover)
      Invoke-Cargo (@('test', '--release', '-j', $Jobs) + (Get-PackageArgs $LbtermTestPackages))
      "Pester ($([math]::Round(((Get-Date) - $pester.Started).TotalSeconds)) s since it started):"
      if (Wait-PesterRun $pester) { throw "Pester: lbterm\tests failed" }
      "Pester, after the build: $(Split-Path -Leaf $afterBuild)"
      $built = Start-PesterRun -Tests $afterBuild -Log (Join-Path $repo 'target\check\pester-after-build.log') -Pwsh $pwsh
      if (Wait-PesterRun $built) { throw "Pester: $(Split-Path -Leaf $afterBuild) failed" }
    } finally { Stop-PesterRun $pester }

    # The exes cargo test doesn't run carry LBTerm's names
    $fail = [Collections.Generic.List[string]]::new()
    foreach ($f in $exes + $extras) {
      if (-not (Test-Path (Join-Path $release $f))) { $fail.Add("missing target\release\$f") }
    }
    # Test and demo windows open with Start-Unfocused (motion\start-unfocused.ps1): Explorer runs
    # a .cmd in a console that comes up in front and takes the keyboard. Only start and restart
    # here use Explorer, to open the real LBTerm.
    foreach ($s in Get-ChildItem $PSScriptRoot -Recurse -Include *.ps1, *.psm1) {
      if ($s.FullName -eq $PSCommandPath) { continue }
      $ast = [System.Management.Automation.Language.Parser]::ParseFile($s.FullName, [ref]$null, [ref]$null)
      $calls = $ast.FindAll({ param($a) $a -is [System.Management.Automation.Language.CommandAst] -and $a.GetCommandName() -match '^explorer(\.exe)?$' }, $true)
      foreach ($c in $calls) { $fail.Add("$($s.Name):$($c.Extent.StartLineNumber) opens a window through Explorer: use Start-Unfocused") }
    }
    $gui = Join-Path $release 'lbterm-gui.exe'
    if (Test-Path $gui) {
      # Only the name: upstream's GUI parses its arguments before env_bootstrap assigns the
      # version, so its --version says "someone forgot to call assign_version_info" (WezTerm too).
      $v = Get-Output $gui '--version'
      if ($v -notmatch '^lbterm-gui ') { $fail.Add("lbterm-gui --version printed '$v'") }
      $info = (Get-Item $gui).VersionInfo
      if ($info.ProductName -ne 'LBTerm') { $fail.Add("lbterm-gui.exe ProductName is '$($info.ProductName)'") }
      if ($info.LegalCopyright -notmatch 'Wez Furlong') { $fail.Add("lbterm-gui.exe lost Wez Furlong's copyright notice") }
    }
    $mux = Join-Path $release 'lbterm-mux-server.exe'
    if (Test-Path $mux) {
      $v = Get-Output $mux '--version'
      if ($v -notmatch '^lbterm-mux-server ') { $fail.Add("lbterm-mux-server --version printed '$v'") }
    }
    # The version names the commit just built (wezterm-version/build.rs). A stale one makes
    # install take the build for the one it replaces, keep no previous, and show no restart pill
    $head = (git -C $repo -c core.abbrev=8 show -s --format=%h HEAD).Trim()
    $built = Get-BuildVersion $release
    if (-not (Test-VersionNamesCommit $built $head)) {
      $fail.Add("lbterm --version says '$built', not HEAD's ${head}: the version stamp is stale")
    }
    if ($fail.Count) { $fail | ForEach-Object { "FAIL $_" }; exit 1 }
    'check passed'
  }

  'test' {
    # A crate's tests on their own, while working on it: the check runs them all
    if (-not $Package) { throw 'test needs -Package <crate>[,<crate>...]' }
    # pwsh -File hands a list over as one string: lbterm-extras,config
    $only = if ($Ignored) { @('--', '--ignored') } else { @() }
    Invoke-Cargo (@('test', '--release', '-j', $Jobs) + (Get-PackageArgs @($Package -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ })) + $only)
  }

  'unify' { Update-Unify }

  'slot' {
    if ($Free) { Clear-LbtermSlot -Main $main -Name $Free; break }
    if (-not $Branch) { throw 'slot needs -Branch <name> (or -Free <slot>)' }
    $r = Request-LbtermSlot -Main $main -Branch $Branch
    if ($r.Path) {
      if ($r.New) {
        "made $($r.Name) with $Branch out: $($r.Path). Its first build is from nothing (11 to 12 minutes); every branch there after that rebuilds only what changed"
      } else {
        "$Branch is out in $($r.Name): $($r.Path). Its build is kept, so cargo rebuilds what changed since (main is $($r.Behind) commit(s) on from that slot's last branch)"
      }
      "Drive it with git -C and full paths, never cd into it. land $Branch keeps the slot and moves it back to main."
      break
    }
    if ($r.Refused) { throw "No slot for $($Branch): $($r.Refused). Wait for a slot to come free (lbterm.ps1 slots), or free some disk." }
    # every slot is busy: a worktree of its own, as before slots
    $busy = ($r.Busy | ForEach-Object { "$($_.Name) has $(if ($_.Branch) { $_.Branch } else { 'uncommitted work or a claim' })" }) -join '; '
    $free = (Get-PSDrive ([IO.Path]::GetPathRoot($main).Substring(0, 1))).Free / 1GB
    if ($free -lt 20) { throw "Every slot is busy ($busy), and a worktree of its own needs 9 to 13 GB once built, with $('{0:N1}' -f $free) GB free. Wait for a slot (lbterm.ps1 slots)." }
    $path = Join-Path $main ".claude\worktrees\$(($Branch -split '/')[-1])"
    if (Test-Path -LiteralPath $path) { throw "Every slot is busy ($busy), and $path is taken: make $Branch's worktree yourself" }
    $exists = -not (Invoke-LbtermGit $main @('rev-parse', '--verify', '--quiet', "refs/heads/$Branch")).Code
    $add = if ($exists) { @('worktree', 'add', '--quiet', $path, $Branch) } else { @('worktree', 'add', '--quiet', '-b', $Branch, $path, 'main') }
    $g = Invoke-LbtermGit $main $add
    if ($g.Code) { throw "git worktree add failed: $($g.Lines -join ' ')" }
    "Every slot is busy ($busy), so $Branch has a worktree of its own: $path. It builds from nothing (11 to 12 minutes), and land removes it as before."
  }

  'slots' {
    foreach ($s in Get-LbtermSlots -Main $main) {
      $state = if (-not $s.Exists) { 'not made yet' }
        elseif (-not $s.IsSlot) { "a worktree that isn't locked as a slot: $(if ($s.Branch) { $s.Branch } else { 'detached' })" }
        elseif ($s.Branch) { "busy: $($s.Branch)" }
        else { "free, on a commit main is $($s.Behind) on from" }
      '{0}  {1}' -f $s.Name, $state
    }
  }

  'prune' {
    $drive = [IO.Path]::GetPathRoot($main).Substring(0, 1)
    $before = (Get-PSDrive $drive).Free
    $total = 0L
    foreach ($t in Get-GitWorktrees $main) {
      if (-not (Test-Path -LiteralPath $t.Path)) { continue }
      $r = Remove-StaleBuildFiles -Tree $t.Path
      $total += $r.Bytes
      Format-StaleBuildFiles $r 'incremental or e2e run'
    }
    "pruned $('{0:N1}' -f ($total / 1GB)) GB in all; ${drive}: has $('{0:N1}' -f ((Get-PSDrive $drive).Free / 1GB)) GB free (it had $('{0:N1}' -f ($before / 1GB)))"
  }

  'install' {
    foreach ($f in $exes + $extras) {
      if (-not (Test-Path (Join-Path $release $f))) { throw "target\release\$f is missing: run build first" }
    }
    $server = Get-Installed lbterm-mux-server
    Assert-ServerReachable (Join-Path $release 'lbterm.exe') 'this build'
    Remove-OldBuilds $install
    $version = Get-BuildVersion $release
    if (Save-Previous -Install $install -Previous $previous -NewVersion $version) {
      "kept the build this replaces ($(Get-BuildVersion $previous)) in ${previous}: lbterm.ps1 rollback goes back to it"
    }
    $moved = Copy-Build -From $release -To $install
    Set-BuildVersion $install $version
    "installed lbterm $version to $install"
    if ($moved) {
      if (Get-Installed lbterm-gui) { 'LBTerm is running the old build until it restarts: lbterm.ps1 restart keeps its tabs when the mux domain is on.' }
      if ($server) { "lbterm-mux-server keeps the old build until its last tab closes." }
    }
  }

  'rollback' {
    if (-not (Test-Path (Join-Path $previous 'lbterm-gui.exe'))) {
      throw "No earlier build is kept in $previous. Install keeps the build it replaces, from the first install after 2026-10-20 on."
    }
    Assert-ServerReachable (Join-Path $previous 'lbterm.exe') 'the kept build'
    Remove-OldBuilds $install
    $from = Get-BuildVersion $install
    $to = Get-BuildVersion $previous
    Switch-Build -Install $install -Previous $previous | Out-Null
    "rolled back from lbterm $from to lbterm $to; lbterm.ps1 rollback again goes forward"
    if (Get-Installed lbterm-gui) {
      try { Restart-Gui } catch { "LBTerm isn't restarted: $($_.Exception.Message)" }
    }
  }

  'restart' { Restart-Gui }

  'try' {
    # This tree's build, in a window beside the daily LBTerm: Lachlan's config, but its own class,
    # its own mux server and socket (New-TryConfig), LBTERM_TRY=1 so Terminal's hooks name its
    # panes' files apart (their ids collide with the daily server's), on the right-hand screen,
    # opened without the keyboard. It runs from a copy in target\try\bin, so builds can still
    # write target\release while it's open.
    Build
    $try = Join-Path $repo 'target\try'
    $bin = Join-Path $try 'bin'
    # The try window this replaces, with its server and their tabs
    $old = @(Get-Process lbterm-gui, lbterm-mux-server -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$bin\*" })
    foreach ($p in $old) { Stop-Process -Id $p.Id -Force; $p.WaitForExit(5000) | Out-Null }
    if ($old) { 'closed the try window this replaces, and its tabs' }
    Remove-OldBuilds $bin
    Copy-Build -From $release -To $bin | Out-Null
    $sock = Join-Path $try 'sock'
    if (Test-Path $sock) { Remove-Item $sock }
    $cfg = Join-Path $try 'try.lua'
    [IO.File]::WriteAllText($cfg, (New-TryConfig -UserConfig (Join-Path $env:USERPROFILE '.lbterm.lua') `
      -Self $cfg -Socket $sock -Server (Join-Path $bin 'lbterm-mux-server.exe')))
    . (Join-Path $PSScriptRoot 'motion\offscreen.ps1')
    $id = Start-Unfocused -Exe (Join-Path $bin 'lbterm-gui.exe') `
      -Arguments "--config-file `"$cfg`" connect try --class $LbtermTryClass --position $TestWindowPosition" `
      -Environment @{ LBTERM_TRY = '1' } -WorkingDirectory $env:USERPROFILE
    if ((Wait-Window -Id $id -Seconds 20) -eq [IntPtr]::Zero) {
      throw "The try window (pid $id) didn't open. Its log: lbterm.ps1 logs -Pid $id ($logDir\lbterm-gui.exe-log-$id.txt)"
    }
    "opened lbterm $(Get-BuildVersion $release) in a try window (pid $id) on the right-hand screen: click it to type. Closing it ends its tabs and its server; the next try replaces it."
  }

  'start' {
    $gui = Join-Path $install 'lbterm-gui.exe'
    if (-not (Test-Path $gui)) { throw "LBTerm isn't installed: run install first" }
    # Through Explorer: a GUI started as a child of Claude's sandboxed shell inherits its job object.
    explorer.exe $gui
  }

  'e2e' {
    # On demand only, never part of the check (Lachlan's choice): test windows take minutes
    Import-Module (Join-Path $PSScriptRoot 'harness.psm1') -Force -DisableNameChecking
    # -Changed lists the candidates and runs nothing: the ones that judge the change are picked
    # from it by name (Get-RunRefusal)
    if ($Changed) { Format-E2eChanged; exit 0 }
    # Keeps the displays on for the run, and turns them back on with a mouse move of no pixels if
    # they're off (Lachlan's E4 and 2a of 2026-11-10), saying so; Get-DisplaysAsleep below then says
    # whether they're on
    Request-DisplaysAwake
    $refusal = Get-RunRefusal -Command e2e -Only $Only -Smoke:$Smoke -AllScripts:$AllScripts -Asleep (Get-DisplaysAsleep)
    if ($refusal) { [Console]::Error.WriteLine($refusal); exit 2 }
    if (-not $Bin) { $Bin = $release }
    if ($TestLog) { $env:LBTERM_E2E_LOG = $TestLog }
    # The runs before this one older than 3 days (Get-StaleRuns), which nothing else removes
    Format-StaleBuildFiles (Remove-StaleBuildFiles -Tree $repo -What runs) 'e2e run'
    Invoke-E2eRun -Bin $Bin -Only $Only -Smoke:$Smoke -Retry $Retry -TimeoutMin $TimeoutMin -WaitIdle:$WaitIdle
    $report = Get-E2eLastReport
    if ($report -and $report.summary.fail) { exit 1 }
  }

  'budgets' {
    # On demand only, like e2e: test windows, and at rest some scenarios fail until fixed
    Import-Module (Join-Path $PSScriptRoot 'harness.psm1') -Force -DisableNameChecking
    Import-Module (Join-Path $PSScriptRoot 'budgets.psm1') -Force -DisableNameChecking
    # Keeps the displays on for the run, and turns them back on with a mouse move of no pixels if
    # they're off (Lachlan's E4 and 2a of 2026-11-10), saying so; Get-DisplaysAsleep below then says
    # whether they're on
    Request-DisplaysAwake
    $refusal = Get-RunRefusal -Command budgets -Only $Only -AllScripts:$AllScripts -Asleep (Get-DisplaysAsleep)
    if ($refusal) { [Console]::Error.WriteLine($refusal); exit 2 }
    if (-not $Bin) { $Bin = $release }
    if ($Against) {
      Invoke-BudgetComparison -Bin $Bin -Build (Get-BuildVersion $Bin) -Against $Against -AgainstBuild (Get-BuildVersion $Against) -Only $Only -Runs $Runs -Seconds $Seconds -Rounds $Rounds -Retry $Retry -TimeoutMin $TimeoutMin -WaitIdle:$WaitIdle
    } else {
      Invoke-BudgetRun -Bin $Bin -Build (Get-BuildVersion $Bin) -Only $Only -Runs $Runs -Seconds $Seconds -Retry $Retry -TimeoutMin $TimeoutMin -WaitIdle:$WaitIdle
    }
    $report = Get-BudgetLastReport
    if (-not $report -or $report.summary.fail -or $report.summary.error) { exit 1 }
  }

  'look' {
    # A look judged by eye (Lachlan's 3b of 2026-11-09): the scripts -Only names on -Against's
    # build and on -Bin's, their captures side by side, for show-image
    Import-Module (Join-Path $PSScriptRoot 'harness.psm1') -Force -DisableNameChecking
    # Keeps the displays on for the run, and turns them back on with a mouse move of no pixels if
    # they're off (Lachlan's E4 and 2a of 2026-11-10), saying so; Get-DisplaysAsleep below then says
    # whether they're on
    Request-DisplaysAwake
    $refusal = Get-RunRefusal -Command look -Only $Only -Against $Against -Asleep (Get-DisplaysAsleep)
    if ($refusal) { [Console]::Error.WriteLine($refusal); exit 2 }
    if (-not $Bin) { $Bin = $release }
    Invoke-LookRun -Bin $Bin -Against $Against -Only $Only -TimeoutMin $TimeoutMin `
      -BeforeLabel "before: $(Get-BuildVersion $Against)" -AfterLabel "after: $(Get-BuildVersion $Bin)"
  }

  'upstream' {
    git -C $repo fetch -q upstream
    if ($LASTEXITCODE) { throw 'git fetch upstream failed' }
    $ahead, $behind = (git -C $repo rev-list --left-right --count main...upstream/main) -split '\s+'
    "main: $ahead commits of LBTerm's own, $behind upstream commits not merged yet"
  }

  'publish' {
    $clone = if ($Public) { $Public } else { Join-Path (Split-Path $main -Parent) 'LBTerm-public' }
    $new = Publish-Lbterm -Repo $repo -Public $clone -Message $Message
    if ($new) { "exported main as $($new.Substring(0, 9)) in $clone" } else { "$clone already has main as it is" }
    if ($Push) {
      git -C $clone push -u origin main
      if ($LASTEXITCODE) { throw "git push from $clone failed" }
    } else {
      $unpushed = @(git -C $clone rev-list main --not --remotes=origin --remotes=upstream 2>$null).Count
      if ($unpushed) { "$unpushed commit(s) not pushed yet: publish -Push sends them" }
    }
  }

  'logs' {
    # Reads the logs, never changes them: a GUI starting deletes those a week old
    $from = if ($Since) { ConvertTo-LbtermSince $Since }
    $all = @(Get-LbtermLogs -Dir $logDir -Install $install -Live (Get-LiveLbterm))
    if ($ProcessId) {
      $logs = @($all | Where-Object Pid -eq $ProcessId)
      if (-not $logs) { throw "No log of pid $ProcessId in $logDir" }
    } else {
      $logs = @(Select-LbtermLogs $all -Kind $Kind -Since $from)
      if ($Latest -or $Follow) { $logs = @($logs | Select-Object -First 1) }
    }
    $kindWord = if ($Kind -and -not $ProcessId) { " $Kind" }
    $sinceWords = if ($from) { " written since $($from.ToString('yyyy-MM-dd HH:mm'))" }
    if (-not $logs) { "No$kindWord logs$sinceWords in $logDir"; break }
    $what = "$($logs.Count)$kindWord log$(if ($logs.Count -ne 1) { 's' })$sinceWords$(if (-not $kindWord) { " ($(Format-LbtermLogKinds $logs))" })"

    if ($Errors) {
      $groups = @(Group-LbtermLogProblems $logs -Since $from)
      "ERROR and WARN lines in $what, grouped by kind, most often first"
      ''
      if ($groups) { Format-LbtermLogProblems $groups } else { 'None.' }
      $slowLines = (@(Group-LbtermSlowFrames $logs -Since $from) | Measure-Object Lines -Sum).Sum
      if ($slowLines) { ''; "Left out: $slowLines slow-frame lines (-Slow)." }
      $budgetLines = (@(Group-LbtermBudgetLines $logs -Since $from) | Measure-Object Lines -Sum).Sum
      if ($budgetLines) { ''; "Left out: $budgetLines budget lines (-Budget)." }
    }
    if ($Slow) {
      $groups = @(Group-LbtermSlowFrames $logs -Since $from)
      if ($Errors) { '' }
      "Slow frames ($LbtermSlowTarget) in $what, by kind and site, the most first"
      ''
      if ($groups) { Format-LbtermSlowFrames $groups } else { 'None.' }
    }
    if ($Budget) {
      $groups = @(Group-LbtermBudgetLines $logs -Since $from)
      if ($Errors -or $Slow) { '' }
      "The budget's lines ($LbtermBudgetTarget) in ${what}: what ran at rest by who asked most, then actions over their time"
      ''
      if ($groups) { Format-LbtermBudget $groups } else { 'None.' }
    }
    if ($Errors -or $Slow -or $Budget) { break }

    if ($Follow) { Watch-Log $logs[0] }
    elseif ($ProcessId -or $Latest) {
      $first = $true
      foreach ($log in $logs) { if (-not $first) { '' }; $first = $false; Show-Log $log $from }
    } else {
      # Newest first; all of them only when -Since says how far back
      $shown = if ($from) { $logs } else { @($logs | Select-Object -First 30) }
      Format-LbtermLogList $shown
      if ($shown.Count -lt $logs.Count) { ''; "The newest $($shown.Count) of $what. -Since 1d (or a time) lists all since then, -Kind one kind." }
    }
  }
}
