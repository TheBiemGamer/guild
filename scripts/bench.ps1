# Scroll benchmark: runs the release build with LAUNCHER_BENCH, prints its frame stats and peak memory.
#   pwsh scripts/bench.ps1 4000
#   pwsh scripts/bench.ps1 15000 -Exe path\to\launcher.exe
param(
  [Parameter(Mandatory)] [int]$Speed,
  [string]$Exe = (Join-Path $PSScriptRoot '..\src-tauri\target\release\launcher.exe')
)
$ErrorActionPreference = 'Stop'

$out = New-TemporaryFile
$env:LAUNCHER_BENCH = $Speed
try {
  $app = Start-Process -FilePath (Resolve-Path $Exe) -RedirectStandardOutput $out -PassThru
} finally {
  Remove-Item Env:LAUNCHER_BENCH
}

# The exe plus every process below it (msedgewebview2 and its helpers).
function Get-Tree([int]$Root) {
  $all = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId
  $ids = [System.Collections.Generic.List[int]]@($Root)
  for ($i = 0; $i -lt $ids.Count; $i++) {
    $all | Where-Object ParentProcessId -eq $ids[$i] | ForEach-Object { $ids.Add([int]$_.ProcessId) }
  }
  $ids
}

$peak = 0
while (-not $app.HasExited) {
  $bytes = 0
  foreach ($id in Get-Tree $app.Id) {
    $p = Get-Process -Id $id -ErrorAction SilentlyContinue
    if ($p) { $bytes += $p.PrivateMemorySize64 }
  }
  $peak = [Math]::Max($peak, $bytes)
  Start-Sleep -Milliseconds 250
}

Get-Content $out | Where-Object { $_ -match '\S' }
Remove-Item $out
'peak private {0:N0} MB' -f ($peak / 1MB)
