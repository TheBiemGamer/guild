# Scroll benchmark: runs the release build with LAUNCHER_BENCH, prints its frame stats and peak memory.
#   pwsh scripts/bench.ps1 4000
#   pwsh scripts/bench.ps1 15000 -Exe path\to\Guild.exe
param(
  [Parameter(Mandatory)] [int]$Speed,
  [string]$Exe = (Join-Path $PSScriptRoot '..\src-tauri\target\release\Guild.exe')
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

# Walking the process list is expensive (a WMI query), so it runs every 5 s only until the tree
# stops changing (WebView2 starts its helpers early); after that only the known PIDs are read,
# so the sampler doesn't compete with the measured scroll.
$peak = 0
$ids = @()
$stable = $false
$walked = [DateTime]::MinValue
while (-not $app.HasExited) {
  if (-not $stable -and ((Get-Date) - $walked).TotalSeconds -ge 5) {
    $now = @(Get-Tree $app.Id | Sort-Object)
    $stable = $ids.Count -gt 1 -and (Compare-Object $ids $now).Count -eq 0
    $ids = $now
    $walked = Get-Date
  }
  $bytes = 0
  foreach ($p in Get-Process -Id $ids -ErrorAction SilentlyContinue) { $bytes += $p.PrivateMemorySize64 }
  $peak = [Math]::Max($peak, $bytes)
  Start-Sleep -Milliseconds 250
}

Get-Content $out | Where-Object { $_ -match '\S' }
Remove-Item $out
'peak private {0:N0} MB' -f ($peak / 1MB)
