#Requires -Version 7
# lbterm\perf\sample.ps1, the sampling profiler: Rust's names demangled, a thread's samples said
# as self and inclusive shares, and a real run against a busy pwsh of the test's own

BeforeAll {
  $script:Sampler = Join-Path $PSScriptRoot '..\perf\sample.ps1'
  . $script:Sampler
}

Describe 'sample.ps1' {
  It 'writes Rust''s legacy names as paths, the hash gone, the escapes written out' {
    ConvertFrom-RustSymbol 'ZN12wezterm_term13terminalstate9performer9Performer11flush_print17h085a3d52e7ac52b1E' | Should -Be 'wezterm_term::terminalstate::performer::Performer::flush_print'
    ConvertFrom-RustSymbol 'ZN114_$LT$wezterm_surface..line..clusterline..ClusterLineCellIter$u20$as$u20$core..iter..traits..iterator..Iterator$GT$4next17h39a778266788c583E' |
      Should -Be '<wezterm_surface::line::clusterline::ClusterLineCellIter as core::iter::traits::iterator::Iterator>::next'
    ConvertFrom-RustSymbol 'ZN5alloc7raw_vec11finish_grow17h01f254eb64036677E.llvm.10923937646640799843' | Should -Be 'alloc::raw_vec::finish_grow'
    # What isn't one comes back as it is
    ConvertFrom-RustSymbol 'NtDeviceIoControlFile' | Should -Be 'NtDeviceIoControlFile'
    ConvertFrom-RustSymbol '(lbterm-gui.exe)' | Should -Be '(lbterm-gui.exe)'
    ConvertFrom-RustSymbol 'ZN3bad99tooshortE' | Should -Be 'ZN3bad99tooshortE'
  }

  It 'says a thread''s samples as the shares a function was on top in, and anywhere in, most first' {
    $samples = @(
      , @('leaf', 'mid', 'root')
      , @('leaf', 'mid', 'root')
      , @('other', 'root')
      , @('mid', 'mid', 'root')
    )
    $lines = @(Get-SampleReport -Thread '7' -Samples $samples)
    $lines[0] | Should -Be '== thread 7, 4 samples'
    $self = $lines[($lines.IndexOf('-- self') + 1)..($lines.IndexOf('-- inclusive') - 1)]
    $self[0] | Should -Match '50\.0%\s+leaf'
    @($self | Where-Object { $_ -match 'mid' })[0] | Should -Match '25\.0%'
    $incl = $lines[($lines.IndexOf('-- inclusive') + 1)..($lines.Count - 1)]
    $incl[0] | Should -Match '100\.0%\s+root'
    # A function twice in one stack counts once there
    @($incl | Where-Object { $_ -match '\smid$' })[0] | Should -Match '75\.0%'
  }

  It 'refuses Lachlan''s own LBTerm and a GUI started without a class' {
    $installed = 'C:\Users\someone\AppData\Local\Programs\LBTerm'
    Get-SampleRefusal -Name 'lbterm-gui' -Path "$installed\lbterm-gui.exe" -CommandLine 'lbterm-gui.exe start --class x' -Installed $installed |
      Should -Match 'installed LBTerm'
    Get-SampleRefusal -Name 'lbterm-mux-server' -Path 'C:/Users/someone/AppData/Local/Programs/LBTerm/lbterm-mux-server.exe' -CommandLine 'lbterm-mux-server.exe --daemonize' -Installed $installed |
      Should -Match 'installed LBTerm'
    Get-SampleRefusal -Name 'lbterm-gui' -Path 'C:\slot\target\release\lbterm-gui.exe' -CommandLine '"C:\slot\target\release\lbterm-gui.exe" start' -Installed $installed |
      Should -Match 'no --class'
    # A test window's, a test's mux server and anything else are sampled
    Get-SampleRefusal -Name 'lbterm-gui' -Path 'C:\slot\target\release\lbterm-gui.exe' -CommandLine '"C:\slot\target\release\lbterm-gui.exe" --config-file c.lua start --position 10,10 --class perf-x' -Installed $installed |
      Should -BeNullOrEmpty
    Get-SampleRefusal -Name 'lbterm-mux-server' -Path 'C:\slot\target\release\lbterm-mux-server.exe' -CommandLine 'lbterm-mux-server.exe --config-file c.lua' -Installed $installed |
      Should -BeNullOrEmpty
    Get-SampleRefusal -Name 'pwsh' -Path 'C:\Program Files\PowerShell\7\pwsh.exe' -CommandLine 'pwsh -NoProfile' -Installed $installed |
      Should -BeNullOrEmpty
    # A folder that only starts like the installed one isn't it
    Get-SampleRefusal -Name 'lbterm-mux-server' -Path "$installed-previous\lbterm-mux-server.exe" -CommandLine 'x' -Installed $installed |
      Should -BeNullOrEmpty
  }

  It 'samples a busy process of its own and names what it finds' {
    $busy = Start-Process 'C:\Program Files\PowerShell\7\pwsh.exe' -ArgumentList '-NoProfile', '-Command', '$n = 0; while ($true) { $n++ }' -WindowStyle Hidden -PassThru
    try {
      Start-Sleep -Milliseconds 1500
      $out = Join-Path $TestDrive 'profile.txt'
      $text = & 'C:\Program Files\PowerShell\7\pwsh.exe' -NoProfile -File $script:Sampler -ProcessId $busy.Id -Seconds 1 -Hz 200 -Threads 1 -PickSec 0.5 -Out $out 2>&1 | Out-String
      $text | Should -Match 'target: pwsh'
      $report = Get-Content -Raw $out
      $report | Should -Match '== thread \d+, \d+ samples'
      [int]([regex]::Match($report, ', (\d+) samples').Groups[1].Value) | Should -BeGreaterThan 20
      $report | Should -Match '-- inclusive'
    } finally {
      Stop-Process -Id $busy.Id -Force -ErrorAction SilentlyContinue
    }
  }
}
