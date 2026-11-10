# Writing a file another process reads, whole: the text into a temp file beside it, then a rename over
# it. Windows refuses that rename for a moment while a reader has the file open without sharing its
# delete (LBTerm reading a state file a test wrote, a consumer polling a producer's file), so the
# rename is tried again every 10 ms for up to 2 s on that refusal and nothing else; past 2 s the temp
# file goes and the error is thrown. Blueprint's README rule 50 (2026-11-09, Lachlan's 11a), the rule
# its statefile.ts keeps for TypeScript. Dot-sourced by harness.psm1 for the perf and motion scripts,
# and by the bench scripts, which run without the harness.

function Move-FileWhole {
  param([Parameter(Mandatory)][string]$From, [Parameter(Mandatory)][string]$To)
  $until = [DateTime]::UtcNow.AddSeconds(2)
  while ($true) {
    try { [IO.File]::Move($From, $To, $true); return }
    catch [System.IO.IOException], [System.UnauthorizedAccessException] {
      if ([DateTime]::UtcNow -ge $until) {
        Remove-Item -LiteralPath $From -Force -ErrorAction SilentlyContinue
        throw
      }
      Start-Sleep -Milliseconds 10
    }
  }
}

function Write-FileWhole {
  param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][AllowEmptyString()][string]$Text, [string]$Temp = "$Path.tmp")
  [IO.File]::WriteAllText($Temp, $Text)
  Move-FileWhole -From $Temp -To $Path
}
