# Pester 5+, run by `lbterm.ps1 check`. Build slots (lbterm.ps1 slot): worktrees under
# .claude\worktrees\slot-<n> that are kept, build and all, and take one branch after another.
# Every repo is a scratch one in TestDrive; nothing here touches LBTerm's own.

BeforeAll {
  . (Join-Path $PSScriptRoot '..\lbterm-lib.ps1')
  # An identity of its own (an earlier file's can be left set, even empty, which git refuses), and
  # no autocrlf; everything put back in AfterAll
  $script:savedEnv = @{}
  $vars = @{
    GIT_CONFIG_COUNT = '1'; GIT_CONFIG_KEY_0 = 'core.autocrlf'; GIT_CONFIG_VALUE_0 = 'false'
    GIT_AUTHOR_NAME = 'Test'; GIT_AUTHOR_EMAIL = 'test@example.com'
    GIT_COMMITTER_NAME = 'Test'; GIT_COMMITTER_EMAIL = 'test@example.com'
  }
  foreach ($k in $vars.Keys) {
    $script:savedEnv[$k] = [Environment]::GetEnvironmentVariable($k)
    Set-Item "Env:$k" $vars[$k]
  }

  # A repo on main with one commit, its worktrees ignored as LBTerm's are
  function New-Repo {
    $r = Join-Path $TestDrive ([guid]::NewGuid().ToString('n').Substring(0, 8))
    New-Item -ItemType Directory -Force $r | Out-Null
    git -C $r init -q -b main
    git -C $r config user.name 'Test'; git -C $r config user.email 'test@example.com'
    Set-Content (Join-Path $r '.gitignore') ".claude/worktrees/`ntarget/"
    Set-Content (Join-Path $r 'a.txt') 'one'
    git -C $r add -A; git -C $r commit -qm init
    $r
  }
  # A commit on main, made in the main checkout
  function Add-MainCommit($r, [string]$text) {
    Set-Content (Join-Path $r 'a.txt') $text
    git -C $r commit -qam $text
  }
  # A slot with nothing out, as land leaves one
  function New-FreeSlot($r, [int]$n) {
    $s = Request-LbtermSlot -Main $r -Branch "tmp/$n-$([guid]::NewGuid().ToString('n').Substring(0, 4))" -FreeGB 100
    Clear-LbtermSlot -Main $r -Name $s.Name | Out-Null
    $s
  }
}

AfterAll {
  foreach ($k in $script:savedEnv.Keys) {
    if ($null -eq $script:savedEnv[$k]) { Remove-Item "Env:$k" -ErrorAction SilentlyContinue } else { Set-Item "Env:$k" $script:savedEnv[$k] }
  }
}

Describe 'Request-LbtermSlot' {
  It 'makes slot-1 with the branch out, locked as a slot, then slot-2 for the next' {
    $r = New-Repo
    $a = Request-LbtermSlot -Main $r -Branch 'feat/a' -FreeGB 100
    $a.Name | Should -Be 'slot-1'
    $a.New | Should -BeTrue
    git -C $a.Path branch --show-current | Should -Be 'feat/a'
    (git -C $r worktree list --porcelain) -join "`n" | Should -Match ([regex]::Escape("locked $LbtermSlotReason"))
    $b = Request-LbtermSlot -Main $r -Branch 'feat/b' -FreeGB 100
    $b.Name | Should -Be 'slot-2'
    (Get-LbtermSlots -Main $r | Where-Object Exists).Branch | Should -Be @('feat/a', 'feat/b')
  }

  It 'takes a free slot again, keeping what git ignores there (its build), with the branch made from main' {
    $r = New-Repo
    $s = New-FreeSlot $r 1
    New-Item -ItemType Directory -Force (Join-Path $s.Path 'target') | Out-Null
    Set-Content (Join-Path $s.Path 'target\built.rlib') 'twelve minutes'
    Add-MainCommit $r 'two'
    $again = Request-LbtermSlot -Main $r -Branch 'feat/next' -FreeGB 100
    $again.Name | Should -Be 'slot-1'
    $again.New | Should -BeFalse
    $again.Behind | Should -Be 1
    Get-Content (Join-Path $s.Path 'target\built.rlib') | Should -Be 'twelve minutes'
    Get-Content (Join-Path $s.Path 'a.txt') | Should -Be 'two'
    git -C $s.Path branch --show-current | Should -Be 'feat/next'
  }

  It 'takes the free slot closest to main, so the least rebuilds' {
    $r = New-Repo
    Request-LbtermSlot -Main $r -Branch 'feat/a' -FreeGB 100 | Out-Null
    Request-LbtermSlot -Main $r -Branch 'feat/b' -FreeGB 100 | Out-Null
    Clear-LbtermSlot -Main $r -Name 'slot-1' | Out-Null
    Add-MainCommit $r 'two'
    Clear-LbtermSlot -Main $r -Name 'slot-2' | Out-Null
    $c = Request-LbtermSlot -Main $r -Branch 'feat/c' -FreeGB 100
    $c.Name | Should -Be 'slot-2'
    $c.Behind | Should -Be 0
  }

  It 'checks out a branch that is there already, and refuses one that is out somewhere' {
    $r = New-Repo
    git -C $r branch 'feat/old'
    $s = Request-LbtermSlot -Main $r -Branch 'feat/old' -FreeGB 100
    git -C $s.Path branch --show-current | Should -Be 'feat/old'
    { Request-LbtermSlot -Main $r -Branch 'feat/old' -FreeGB 100 } | Should -Throw '*checked out already*'
  }

  It 'says every slot is busy, and refuses to make one on a full disk' {
    $r = New-Repo
    Request-LbtermSlot -Main $r -Branch 'feat/a' -Count 2 -FreeGB 100 | Out-Null
    Request-LbtermSlot -Main $r -Branch 'feat/b' -Count 2 -FreeGB 100 | Out-Null
    $busy = Request-LbtermSlot -Main $r -Branch 'feat/c' -Count 2 -FreeGB 100
    $busy.Path | Should -BeNullOrEmpty
    @($busy.Busy).Count | Should -Be 2
    $full = Request-LbtermSlot -Main (New-Repo) -Branch 'feat/d' -FreeGB 5
    $full.Refused | Should -Match '5\.0 GB is free'
  }

  It 'leaves a slot another session is taking, and takes one whose claim died' {
    $r = New-Repo
    $s = New-FreeSlot $r 1
    $claim = Join-Path (git -C $s.Path rev-parse --absolute-git-dir) 'lbterm-slot-claim'
    Set-Content $claim 'pid 1'
    (Request-LbtermSlot -Main $r -Branch 'feat/x' -FreeGB 100).Name | Should -Be 'slot-2'
    [IO.File]::SetLastWriteTime($claim, (Get-Date).AddMinutes(-30))
    (Request-LbtermSlot -Main $r -Branch 'feat/y' -FreeGB 100).Name | Should -Be 'slot-1'
    Test-Path $claim | Should -BeFalse
  }
}

Describe 'Clear-LbtermSlot' {
  It 'puts a slot back on main with nothing out, keeping the branch, and refuses uncommitted work' {
    $r = New-Repo
    $s = Request-LbtermSlot -Main $r -Branch 'feat/a' -FreeGB 100
    Set-Content (Join-Path $s.Path 'a.txt') 'work'
    { Clear-LbtermSlot -Main $r -Name 'slot-1' } | Should -Throw '*uncommitted*'
    git -C $s.Path commit -qam 'work'
    Clear-LbtermSlot -Main $r -Name 'slot-1' | Should -Match 'feat/a stays, with 1 commit'
    git -C $s.Path symbolic-ref -q HEAD | Should -BeNullOrEmpty
    git -C $s.Path rev-parse HEAD | Should -Be (git -C $r rev-parse main)
    (Get-LbtermSlots -Main $r)[0].Free | Should -BeTrue
    Clear-LbtermSlot -Main $r -Name 'slot-1' | Should -Match 'free already'
  }

  It "refuses a worktree that isn't a slot" {
    $r = New-Repo
    $wt = Join-Path $r '.claude\worktrees\slot-1'
    git -C $r worktree add -q -b 'feat/plain' $wt 2>$null
    { Clear-LbtermSlot -Main $r -Name 'slot-1' } | Should -Throw "*isn't a slot*"
  }
}
