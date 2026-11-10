# Console Host

This directory contains a copy of built artifacts from the Microsoft
Terminal project which is provided by Microsoft under the terms
of the MIT license.

Why are they here?  At the time of writing, the conpty implementation
that ships with windows is lacking support for mouse reporting but
that support is available in the opensource project so it is desirable
to point to that so that we can enable mouse reporting in wezterm.

It looks like we'll eventually be able to drop this once Windows
and/or the build for the terminal project make some more progress.

https://github.com/wezterm/wezterm/issues/1927

These assets were built by cloning the ms-terminal repo and running:

```
.\tools\razzle.cmd
bcz rel
```

then the files can be copied from `bin/x64/Release` to this location.

It's possible that you'll need to download this runtime support package
from MS in order for this to work:
https://www.microsoft.com/en-us/download/details.aspx?id=53175

LBTerm: since 2026-10-31 these are 1.25.2609.30003, the x64 files of
Microsoft's `Microsoft.Windows.Console.ConPTY` package 1.25.260930003 from
nuget.org (`runtimes/win-x64/native/conpty.dll` and
`build/native/runtimes/x64/OpenConsole.exe`, both signed by Microsoft
Corporation), where upstream's 1.22.2502.04002 came from the same package's
1.22.250204002. Measured in the same LBTerm build, they take output with
dense SGR colour 19 to 24% faster with about a quarter less of their own
CPU, and everything else the same (lbterm/BENCH.md, "ConPTY 1.22, 1.24 and
1.25").
