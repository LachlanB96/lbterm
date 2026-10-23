# Pester 5+, run by `lbterm.ps1 check`. The public repo's export (Publish-Lbterm) and its leak check
# (lbterm/publish/leak-check.sh, run by Git's bash as the clone's hooks run it), in throwaway repos
# under TestDrive: a stand-in upstream, a private repo built on it, and a public clone. The
# blocklist is made up and gitleaks is a stand-in, so nothing here reads the real lists or scans
# the real repo.

BeforeDiscovery {
  $hasBash = Test-Path 'C:\Program Files\Git\bin\bash.exe'
}

BeforeAll {
  $ErrorActionPreference = 'Stop'
  . (Join-Path $PSScriptRoot '..\lbterm-lib.ps1')
  $bash = 'C:\Program Files\Git\bin\bash.exe'
  $publishDir = (Resolve-Path (Join-Path $PSScriptRoot '..\publish')).Path
  $check = (Join-Path $publishDir 'leak-check.sh') -replace '\\', '/'

  # A stand-in gitleaks on PATH ahead of the real one: passes, unless a test makes it fail
  $bin = Join-Path $TestDrive 'bin'
  New-Item -ItemType Directory $bin | Out-Null
  function Set-Gitleaks([int]$Exit) {
    [IO.File]::WriteAllText((Join-Path $bin 'gitleaks'), "#!/usr/bin/env bash`nexit $Exit`n")
  }
  Set-Gitleaks 0
  $nohooks = Join-Path $TestDrive 'nohooks'
  New-Item -ItemType Directory $nohooks | Out-Null

  $saved = @{}
  $envs = @{
    PATH                           = "$bin;$env:PATH"
    LBTERM_BLOCKLIST_FILE          = Join-Path $TestDrive 'blocklist.txt'
    LBTERM_ALLOWED_IDENTITIES_FILE = Join-Path $TestDrive 'allowed.txt'
    # A public identity for every commit, unless a test says otherwise
    GIT_AUTHOR_NAME                = 'octo'
    GIT_AUTHOR_EMAIL               = '1+octo@users.noreply.github.com'
    GIT_COMMITTER_NAME             = 'octo'
    GIT_COMMITTER_EMAIL            = '1+octo@users.noreply.github.com'
  }
  foreach ($k in $envs.Keys) {
    $saved[$k] = [Environment]::GetEnvironmentVariable($k)
    [Environment]::SetEnvironmentVariable($k, $envs[$k])
  }
  # Made-up personal strings, with the padding, comment, blank line and CR a hand-edited list can have
  [IO.File]::WriteAllText($env:LBTERM_BLOCKLIST_FILE, "# made-up personal strings`n  Zanzibar Quill `nquill@example.org`r`n`n")
  [IO.File]::WriteAllText($env:LBTERM_ALLOWED_IDENTITIES_FILE, '')

  # A fresh repo on main with no hooks: not this PC's shared ones, nor the global config's
  function New-Repo([string]$Name) {
    $d = Join-Path $TestDrive "$Name-$([guid]::NewGuid().ToString('N').Substring(0, 8))"
    git init -q -b main $d
    git -C $d config core.hooksPath $nohooks
    git -C $d config core.autocrlf false
    $d
  }

  function Add-File([string]$Repo, [string]$Path, [string]$Text) {
    $f = Join-Path $Repo $Path
    New-Item -ItemType Directory -Force (Split-Path $f) | Out-Null
    [IO.File]::WriteAllText($f, "$Text`n")
    git -C $Repo add -- $Path
  }

  # The check run where the hooks run it, from the repo's top: its exit code and all it printed
  function Invoke-LeakCheck([string]$Repo, [string[]]$Arguments) {
    Push-Location $Repo
    try {
      $out = & $bash $check @Arguments 2>&1 | Out-String
      [pscustomobject]@{ Code = $LASTEXITCODE; Out = $out }
    } finally { Pop-Location }
  }

  # What the check printed must never show the matched text
  function Assert-Quiet($r) {
    $r.Out | Should -Not -Match '(?i)zanzibar|quill'
  }

  # A stand-in wezterm/wezterm, a private LBTerm on it (committed as the blocklisted Zanzibar Quill,
  # so a private commit in the public repo can't go unseen), and an empty public clone set up as
  # the real one is, with a bare origin to push to
  function New-Setup {
    $up = New-Repo 'upstream'
    Add-File $up 'README.md' 'WezTerm'
    Add-File $up 'src/a.rs' 'fn main() {}'
    git -C $up commit -q -m 'Upstream one'

    $priv = New-Repo 'private'
    git -C $priv remote add upstream $up
    git -C $priv fetch -q upstream
    git -C $priv reset -q --hard upstream/main
    $env:GIT_AUTHOR_NAME = 'Zanzibar Quill'; $env:GIT_AUTHOR_EMAIL = 'quill@example.org'
    try {
      git -C $priv switch -q -c feat/one
      Add-File $priv 'lbterm/one.ps1' 'one'
      Add-File $priv 'CLAUDE.md' 'notes for Claude: C:\Users\zanzibarquill'
      Add-File $priv '.claude/memory/MEMORY.md' 'memory'
      foreach ($f in Get-ChildItem -Recurse -File $publishDir) {
        Add-File $priv ([IO.Path]::GetRelativePath((Split-Path (Split-Path $publishDir)), $f.FullName) -replace '\\', '/') (Get-Content -Raw $f.FullName).TrimEnd("`n")
      }
      # A submodule, as WezTerm's deps are: it has to cross as the commit it is
      git -C $priv update-index --add --cacheinfo "160000,$((git -C $up rev-parse HEAD)),deps/sub"
      git -C $priv commit -q -m 'One'
      git -C $priv switch -q main
      git -C $priv merge -q --no-ff feat/one -m 'Merge feat/one: the first feature'
    } finally {
      $env:GIT_AUTHOR_NAME = 'octo'; $env:GIT_AUTHOR_EMAIL = '1+octo@users.noreply.github.com'
    }

    $origin = Join-Path $TestDrive "origin-$([guid]::NewGuid().ToString('N').Substring(0, 8)).git"
    git init -q --bare -b main $origin
    $pub = New-Repo 'public'
    git -C $pub config core.hooksPath lbterm/publish/hooks
    git -C $pub config user.name octo
    git -C $pub config user.email '1+octo@users.noreply.github.com'
    git -C $pub remote add origin $origin
    [pscustomobject]@{ Upstream = $up; Private = $priv; Public = $pub; Origin = $origin }
  }
}

AfterAll {
  foreach ($k in $saved.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) }
}

Describe 'leak-check.sh' -Skip:(-not $hasBash) {
  BeforeEach {
    Set-Gitleaks 0
    [IO.File]::WriteAllText($env:LBTERM_ALLOWED_IDENTITIES_FILE, '')
    $d = New-Repo 'leaks'
  }

  It 'takes a mode' {
    (Invoke-LeakCheck $d @()).Code | Should -Be 2
    (Invoke-LeakCheck $d @('--msg')).Code | Should -Be 2
  }

  It 'fails closed without a blocklist' {
    $list = $env:LBTERM_BLOCKLIST_FILE
    $env:LBTERM_BLOCKLIST_FILE = Join-Path $TestDrive 'missing.txt'
    try {
      $r = Invoke-LeakCheck $d @('--staged')
    } finally { $env:LBTERM_BLOCKLIST_FILE = $list }
    $r.Code | Should -Be 1
    $r.Out | Should -Match 'no blocklist entries'
  }

  It 'passes a clean staged file' {
    Add-File $d 'a.md' 'hello garden'
    (Invoke-LeakCheck $d @('--staged')).Code | Should -Be 0
  }

  It 'fails staged contents with a blocklisted string in any case, naming the file but not the string' {
    Add-File $d 'notes.md' 'met zanzibar QUILL today'
    $r = Invoke-LeakCheck $d @('--staged')
    $r.Code | Should -Be 1
    $r.Out | Should -Match 'notes\.md'
    Assert-Quiet $r
  }

  It 'fails a blocklisted path without printing it' {
    Add-File $d 'Zanzibar Quill/x.md' 'x'
    $r = Invoke-LeakCheck $d @('--staged')
    $r.Code | Should -Be 1
    $r.Out | Should -Match 'path contains a blocklisted string'
    Assert-Quiet $r
  }

  It 'fails a personal author, unless that exact identity is allowed' {
    Add-File $d 'a.md' 'hello garden'
    $env:GIT_AUTHOR_EMAIL = 'quill@example.org'
    try {
      $r = Invoke-LeakCheck $d @('--staged')
      $r.Code | Should -Be 1
      $r.Out | Should -Match 'author or committer'
      [IO.File]::WriteAllText($env:LBTERM_ALLOWED_IDENTITIES_FILE, "OCTO <Quill@example.org>`n")
      (Invoke-LeakCheck $d @('--staged')).Code | Should -Be 0
      $env:GIT_AUTHOR_NAME = 'Zanzibar'
      (Invoke-LeakCheck $d @('--staged')).Code | Should -Be 1
    } finally {
      $env:GIT_AUTHOR_NAME = 'octo'; $env:GIT_AUTHOR_EMAIL = '1+octo@users.noreply.github.com'
    }
  }

  It 'fails on a gitleaks finding' {
    Add-File $d 'a.md' 'hello garden'
    Set-Gitleaks 1
    $r = Invoke-LeakCheck $d @('--staged')
    $r.Code | Should -Be 1
    $r.Out | Should -Match 'gitleaks found a possible secret'
  }

  It 'checks a commit message, but not its comments or the diff under the scissors' {
    $m = Join-Path $TestDrive 'msg'
    [IO.File]::WriteAllText($m, "Add the garden`n")
    (Invoke-LeakCheck $d @('--msg', $m)).Code | Should -Be 0
    [IO.File]::WriteAllText($m, "Thanks, Zanzibar Quill`n")
    $r = Invoke-LeakCheck $d @('--msg', $m)
    $r.Code | Should -Be 1
    $r.Out | Should -Match 'commit message contains'
    Assert-Quiet $r
    [IO.File]::WriteAllText($m, "Add the garden`n# zanzibar quill in a comment`n# ------------------------ >8 ------------------------`nquill@example.org in the diff`n")
    (Invoke-LeakCheck $d @('--msg', $m)).Code | Should -Be 0
  }

  It 'refuses --all without an upstream ref to leave out' {
    Add-File $d 'a.md' 'garden'
    git -C $d commit -q -m 'first'
    $r = Invoke-LeakCheck $d @('--all')
    $r.Code | Should -Be 1
    $r.Out | Should -Match 'refs/remotes/upstream'
  }

  It 'leaves out the upstream history and checks every commit after it, deleted files too' {
    Add-File $d 'a.md' 'zanzibar quill, upstream'
    $env:GIT_AUTHOR_NAME = 'Zanzibar Quill'
    try { git -C $d commit -q -m 'upstream, by Zanzibar Quill' } finally { $env:GIT_AUTHOR_NAME = 'octo' }
    git -C $d update-ref refs/remotes/upstream/base HEAD
    git -C $d rm -q a.md
    Add-File $d 'b.md' 'garden'
    git -C $d commit -q -m 'ours'
    $r = Invoke-LeakCheck $d @('--all')
    $r.Code | Should -Be 0
    $r.Out | Should -Match 'checked 1 commit'

    Add-File $d 'c.md' 'Zanzibar Quill'
    git -C $d commit -q -m 'second'
    git -C $d rm -q c.md
    git -C $d commit -q -m 'third'
    $r = Invoke-LeakCheck $d @('--all')
    $r.Code | Should -Be 1
    $r.Out | Should -Match 'files contain blocklisted strings'
    Assert-Quiet $r
  }

  It 'fails a personal author or message after the upstream history' {
    Add-File $d 'a.md' 'garden'
    git -C $d commit -q -m 'upstream'
    git -C $d update-ref refs/remotes/upstream/base HEAD
    Add-File $d 'b.md' 'garden'
    git -C $d commit -q -m 'for zanzibar quill'
    $r = Invoke-LeakCheck $d @('--all')
    $r.Code | Should -Be 1
    $r.Out | Should -Match 'message contains a blocklisted string'
    Assert-Quiet $r
  }
}

Describe 'Get-PublicTree' {
  It 'leaves out what Claude reads and keeps the rest, submodules as they are' {
    $s = New-Setup
    $tree = Get-PublicTree -Repo $s.Private -Commit main
    $entries = git -C $s.Private ls-tree -r $tree
    $names = $entries | ForEach-Object { ($_ -split "`t", 2)[1] }
    $names | Should -Contain 'lbterm/one.ps1'
    $names | Should -Contain 'README.md'
    $names | Should -Not -Contain 'CLAUDE.md'
    $names | Where-Object { $_ -like '.claude/*' } | Should -BeNullOrEmpty
    $entries | Where-Object { $_ -match '^160000 commit [0-9a-f]+\tdeps/sub$' } | Should -HaveCount 1
    # The checkout's own index is untouched
    git -C $s.Private diff --cached --name-only | Should -BeNullOrEmpty
    Test-Path (Join-Path (git -C $s.Private rev-parse --absolute-git-dir) 'lbterm-publish.index') | Should -BeFalse
  }
}

Describe 'Get-PublishMessage' {
  It 'says what LBTerm is the first time, then lists the merges since' {
    $s = New-Setup
    $now = [datetime]'2026-10-23'
    $main = git -C $s.Private rev-parse main
    $first = Get-PublishMessage -Repo $s.Private -Commit $main -Now $now
    $first | Should -Match "^LBTerm, 2026-10-23`n`nLachlan Brown's fork of WezTerm"
    $first | Should -Match "`n`nLBTerm-Commit: $main$"

    git -C $s.Private switch -q -c feat/two
    Add-File $s.Private 'lbterm/two.ps1' 'two'
    git -C $s.Private commit -q -m 'Two'
    git -C $s.Private switch -q main
    git -C $s.Private merge -q --no-ff feat/two -m 'Merge feat/two: the second feature'
    $next = Get-PublishMessage -Repo $s.Private -Commit main -Since $main -Now $now
    $next | Should -Match "`n- the second feature`n"
    $next | Should -Not -Match 'first feature'
  }
}

Describe 'Publish-Lbterm' -Skip:(-not $hasBash) {
  It 'exports main onto the upstream base, and nothing of the private history' {
    $s = New-Setup
    $base = git -C $s.Upstream rev-parse HEAD
    $new = Publish-Lbterm -Repo $s.Private -Public $s.Public -Now ([datetime]'2026-10-23')
    $new | Should -Be (git -C $s.Public rev-parse HEAD)
    (git -C $s.Public rev-list --parents -n 1 HEAD) -split ' ' | Select-Object -Skip 1 | Should -Be @($base)
    git -C $s.Public rev-parse 'HEAD^{tree}' | Should -Be (Get-PublicTree -Repo $s.Private -Commit main)
    Test-Path (Join-Path $s.Public 'lbterm/one.ps1') | Should -BeTrue
    Test-Path (Join-Path $s.Public 'CLAUDE.md') | Should -BeFalse
    git -C $s.Public log -1 --format=%B | Should -Contain "LBTerm-Commit: $(git -C $s.Private rev-parse main)"
    git -C $s.Public rev-parse refs/remotes/upstream/base | Should -Be $base
    # The fetch's ref is gone from the private repo, and no commit of its own came across
    git -C $s.Private for-each-ref refs/lbterm | Should -BeNullOrEmpty
    git -C $s.Public log --all --format='%an %ae %cn %ce' | Should -Not -Match '(?i)zanzibar|quill'
    git -C $s.Public status --porcelain | Should -BeNullOrEmpty
  }

  It 'pushes past the pre-push check, sending only upstream history and the export' {
    $s = New-Setup
    Publish-Lbterm -Repo $s.Private -Public $s.Public | Out-Null
    git -C $s.Public push -q origin main 2>&1 | Out-Null
    $LASTEXITCODE | Should -Be 0
    git -C $s.Origin rev-parse main | Should -Be (git -C $s.Public rev-parse main)
    git -C $s.Origin log --all --format='%an %ae %cn %ce' | Should -Not -Match '(?i)zanzibar|quill'
  }

  It 'changes nothing when main is as it was exported' {
    $s = New-Setup
    $first = Publish-Lbterm -Repo $s.Private -Public $s.Public
    Publish-Lbterm -Repo $s.Private -Public $s.Public | Should -BeNullOrEmpty
    git -C $s.Public rev-parse HEAD | Should -Be $first
  }

  It 'puts a newer upstream base in as a second parent, and lists the merges since' {
    $s = New-Setup
    $first = Publish-Lbterm -Repo $s.Private -Public $s.Public
    Add-File $s.Upstream 'src/b.rs' 'fn b() {}'
    git -C $s.Upstream commit -q -m 'Upstream two'
    $base2 = git -C $s.Upstream rev-parse HEAD
    git -C $s.Private fetch -q upstream
    git -C $s.Private merge -q --no-ff upstream/main -m 'Merge chore/upstream: WezTerm up to two'
    $second = Publish-Lbterm -Repo $s.Private -Public $s.Public
    (git -C $s.Public rev-list --parents -n 1 $second) -split ' ' | Select-Object -Skip 1 | Should -Be @($first, $base2)
    Test-Path (Join-Path $s.Public 'src/b.rs') | Should -BeTrue
    git -C $s.Public log -1 --format=%B | Should -Contain '- WezTerm up to two'
    git -C $s.Public rev-parse refs/remotes/upstream/base | Should -Be $base2
    (Invoke-LeakCheck $s.Public @('--all')).Out | Should -Match 'checked 2 commit'
  }

  It 'leaves the clone as it was when its hooks refuse an export' {
    $s = New-Setup
    $first = Publish-Lbterm -Repo $s.Private -Public $s.Public
    Add-File $s.Private 'lbterm/two.ps1' 'for Zanzibar Quill'
    git -C $s.Private commit -q -m 'Two'
    { Publish-Lbterm -Repo $s.Private -Public $s.Public 6>$null } | Should -Throw '*refused the export*'
    git -C $s.Public rev-parse HEAD | Should -Be $first
    git -C $s.Public status --porcelain | Should -BeNullOrEmpty
    Test-Path (Join-Path $s.Public 'lbterm/two.ps1') | Should -BeFalse
  }

  It 'leaves an empty clone empty when its hooks refuse the first export' {
    $s = New-Setup
    Add-File $s.Private 'lbterm/two.ps1' 'for Zanzibar Quill'
    git -C $s.Private commit -q -m 'Two'
    { Publish-Lbterm -Repo $s.Private -Public $s.Public } | Should -Throw '*refused the export*'
    git -C $s.Public rev-parse --verify -q HEAD | Should -BeNullOrEmpty
    git -C $s.Public ls-files | Should -BeNullOrEmpty
    Get-ChildItem -Force $s.Public | Where-Object Name -ne '.git' | Should -BeNullOrEmpty
  }

  It 'refuses a clone without its hooks, with a personal identity, or with changes of its own' {
    $s = New-Setup
    git -C $s.Public config core.hooksPath $nohooks
    { Publish-Lbterm -Repo $s.Private -Public $s.Public } | Should -Throw '*hooks*'
    git -C $s.Public config core.hooksPath lbterm/publish/hooks
    git -C $s.Public config user.email 'quill@example.org'
    { Publish-Lbterm -Repo $s.Private -Public $s.Public } | Should -Throw '*noreply*'
    git -C $s.Public config user.email '1+octo@users.noreply.github.com'
    Add-File $s.Public 'stray.md' 'stray'
    { Publish-Lbterm -Repo $s.Private -Public $s.Public } | Should -Throw '*changes of its own*'
  }
}
