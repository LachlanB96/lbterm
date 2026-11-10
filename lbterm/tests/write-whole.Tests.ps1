# Pester 5+. lbterm/write-whole.ps1: a file written whole through a rename that waits out a reader
# holding the file open, as Windows refuses the rename then (Blueprint's README rule 50).

BeforeAll {
  . (Join-Path $PSScriptRoot '..\write-whole.ps1')
  Add-Type @'
using System.IO;
using System.Threading;
// A reader that holds a file open without sharing its delete, so a rename over it is refused,
// and lets go after a while on a thread of its own
public static class Holder {
  public static Thread Hold(string path, int ms) {
    var f = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read);
    var t = new Thread(() => { Thread.Sleep(ms); f.Dispose(); });
    t.Start();
    return t;
  }
}
'@
}

Describe 'Write-FileWhole' {
  It 'writes the text whole and leaves no temp file' {
    $f = Join-Path $TestDrive 'a.json'
    Write-FileWhole -Path $f -Text '{"n":1}'
    Get-Content $f -Raw | Should -Be '{"n":1}'
    "$f.tmp" | Should -Not -Exist
    Write-FileWhole -Path $f -Text '{"n":2}'
    Get-Content $f -Raw | Should -Be '{"n":2}'
  }

  It 'takes a temp name of its own and an empty text' {
    $f = Join-Path $TestDrive 'b.txt'
    Write-FileWhole -Path $f -Text '' -Temp "$f.part"
    (Get-Item $f).Length | Should -Be 0
    "$f.part" | Should -Not -Exist
  }

  It 'waits out a reader holding the file open, and then writes' {
    $f = Join-Path $TestDrive 'held.json'
    [IO.File]::WriteAllText($f, 'old')
    $t = [Holder]::Hold($f, 300)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    Write-FileWhole -Path $f -Text 'new'
    $sw.ElapsedMilliseconds | Should -BeGreaterThan 200 -Because 'the rename was refused until the reader let go'
    $t.Join()
    Get-Content $f -Raw | Should -Be 'new'
  }

  It 'gives up after 2 s, taking its temp file away, and throws' {
    $f = Join-Path $TestDrive 'stuck.json'
    [IO.File]::WriteAllText($f, 'old')
    $t = [Holder]::Hold($f, 3000)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    { Write-FileWhole -Path $f -Text 'new' } | Should -Throw
    $sw.ElapsedMilliseconds | Should -BeGreaterOrEqual 1900
    "$f.tmp" | Should -Not -Exist
    $t.Join()
    Get-Content $f -Raw | Should -Be 'old'
  }

  It "doesn't wait on anything but a refused rename: a folder that isn't there fails at once" {
    $f = Join-Path $TestDrive 'no\such\folder\c.json'
    $sw = [Diagnostics.Stopwatch]::StartNew()
    { Write-FileWhole -Path $f -Text 'x' } | Should -Throw
    $sw.ElapsedMilliseconds | Should -BeLessThan 1000
  }
}
