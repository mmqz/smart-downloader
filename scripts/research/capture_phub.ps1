# capture_phub.ps1 - capture real pr-phub.sandai.net:80 HTTP POSTs via pktmon
# (no Wireshark / Npcap install needed; pktmon ships with Windows).
#
# Usage: run AS ADMINISTRATOR. Then open Xunlei and start any BT/magnet download,
# wait a few seconds, press Enter to stop. Outputs pcapng + text dump.
#
# Target facts (from docs/research/xunlei/p2p_research_complete.md):
#   pr-phub.sandai.net -> 140.206.220.33 (Shanghai Telecom), TCP 80 plaintext HTTP,
#   POST / returns "decrypt request failed" when body is wrong.
$ErrorActionPreference = 'Stop'
$phubIp = '140.206.220.33'
$outDir = Join-Path $PSScriptRoot 'captures'
New-Item -ItemType Directory -Force $outDir | Out-Null
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$pktlog = Join-Path $outDir "phub-$stamp.etl"
$pcap = Join-Path $outDir "phub-$stamp.pcapng"
$txt = Join-Path $outDir "phub-$stamp.txt"

$admin = (New-Object Security.Principal.WindowsPrincipal(
    [Security.Principal.WindowsIdentity]::GetCurrent()
)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) {
    Write-Host '[ERR] Admin required: right-click PowerShell -> Run as administrator'
    exit 1
}

pktmon filter remove *> $null
pktmon filter add phub -t TCP -i $phubIp -p 80
if ($LASTEXITCODE -ne 0) { throw 'filter add failed' }
pktmon start --capture --pkt-size 0
if ($LASTEXITCODE -ne 0) { throw 'pktmon start failed' }

Write-Host "[OK] capturing 140.206.220.33:80 (TCP) -> $pcap"
Write-Host ''
Write-Host 'Now: open Xunlei, start ONE BT/magnet download, wait 5-10s.'
$null = Read-Host 'Press Enter when done'
pktmon stop
if ($LASTEXITCODE -ne 0) { throw 'pktmon stop failed' }
pktmon etl2pcap $pktlog -o $pcap
pktmon etl2txt $pktlog -o $txt
Write-Host ''
Write-Host "[DONE] $pcap"
Write-Host "       $txt"
Write-Host 'Send the .pcapng (or .txt) path to the agent for body extraction.'