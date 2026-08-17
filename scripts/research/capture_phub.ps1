# capture_phub.ps1 - capture ALL traffic while Xunlei does a BT download, then
# the agent off-line extracts PHub HTTP POSTs from the pcapng.
# (No Wireshark / Npcap install: pktmon ships with Windows. Run as admin or
# accept the UAC prompt; the script self-elevates.)
#
# v2: no IP filter (pr-phub's IP rotates / version-specific), capture everything
# for ~30s; agent filters afterwards. Also fixes -f log path (v1 wrote to the
# default C:\WINDOWS\system32\PktMon.etl and the convert step failed).
$ErrorActionPreference = 'Stop'
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
    # self-elevate via UAC (user clicks Yes once)
    $argList = '-ExecutionPolicy Bypass -NoExit -File "' + $MyInvocation.MyCommand.Path + '"'
    try {
        Start-Process powershell -Verb RunAs -ArgumentList $argList
    } catch {
        Write-Host '[ERR] UAC elevation cancelled or failed. Re-run in an admin terminal.'
        exit 1
    }
    exit 0
}

pktmon filter remove *> $null
pktmon start --capture --pkt-size 0 -f $pktlog
if ($LASTEXITCODE -ne 0) { throw 'pktmon start failed' }

Write-Host "[OK] capturing ALL traffic -> $pcap"
Write-Host ''
Write-Host 'Now: open Xunlei, start ONE BT/magnet download, wait 20-30s.'
Write-Host '(If no BT task at hand: open a magnet/BT task in the download list.)'
$null = Read-Host 'Press Enter when done'
pktmon stop
if ($LASTEXITCODE -ne 0) { throw 'pktmon stop failed' }
pktmon etl2pcap $pktlog -o $pcap
pktmon etl2txt $pktlog -o $txt
Write-Host ''
Write-Host "[DONE] $pcap"
Write-Host "       $txt"
Write-Host 'Send the .pcapng path to the agent for extraction.'
$null = Read-Host 'Press Enter to close'