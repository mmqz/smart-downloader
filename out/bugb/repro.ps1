# Bug B 免配额复现脚本 v2（慢速 Mock 延迟 Ready + 本地 seeder 直连 + python 静态源）
# 用法: pwsh -File repro.ps1 -Tag A|B [-DelaySecs 240] [-DownLimitKbS 64]
#   Tag A = 卡内处方分支：metadata 到达即 pause，BT 在 poll_ready 等待窗内自行完成进 Seeding
#   Tag B = 对比分支：BT 先完成(done==total)静置后再 pause/fallback
param(
    [string]$Tag = 'A',
    [int]$DelaySecs = 240,
    [int]$DownLimitKbS = 64
)
$ErrorActionPreference = 'Continue'
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$root  = Join-Path $PSScriptRoot ("run-{0}-{1}" -f $Tag, $stamp)
New-Item -ItemType Directory -Force -Path $root, (Join-Path $root 'downloads'), (Join-Path $root 'seeddir') | Out-Null

$apiPort  = 18000 + (Get-Random -Maximum 4000)
$httpPort = 19000 + (Get-Random -Maximum 4000)
$seedPort = 16900 + (Get-Random -Maximum 400)
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Clean-All {
    foreach ($pr in @($daemon, $seedProc, $httpProc)) {
        if ($pr -and -not $pr.HasExited) { Stop-Process -Id $pr.Id -Force -ErrorAction SilentlyContinue }
    }
}

# ---- 配置文件（provider=mock，不启用 xunlei；TOML 路径用正斜杠）----
$rootF = $root -replace '\\', '/'
$cfg = @"
[server]
addr = "127.0.0.1:$apiPort"

[download]
dest_root = "$rootF/downloads"
max_download_kb_s = $DownLimitKbS

[bt]
enabled = true

[provider]
enabled = true
mock = true

[storage]
tasks_path = "$rootF/tasks.json"

[lock]
path = "$rootF/daemon.lock"
"@
Set-Content -Path (Join-Path $root 'config.toml') -Value $cfg -Encoding UTF8

# ---- mock 直链载荷（2MB 确定性内容）----
$payload = Join-Path $root 'mockfile.bin'
$b = New-Object byte[] (2MB)
(New-Object Random(1234)).NextBytes($b)
[IO.File]::WriteAllBytes($payload, $b)

# ---- 本地 HTTP 源（range_server.py：支持 206 多段下载）----
$python = (Get-Command python -ErrorAction SilentlyContinue).Source
if (-not $python) { Write-Host 'FATAL: python 不可用'; exit 2 }
$httpProc = Start-Process -FilePath $python `
    -ArgumentList (Join-Path $PSScriptRoot 'range_server.py'), "$httpPort", $root `
    -WorkingDirectory $root -RedirectStandardOutput (Join-Path $root 'http.log') -RedirectStandardError (Join-Path $root 'http.err') `
    -PassThru -WindowStyle Hidden
$httpUp = $false
foreach ($i in 1..50) {
    if ($httpProc.HasExited) { break }
    try { Invoke-WebRequest "http://127.0.0.1:$httpPort/mockfile.bin" -Method Head -TimeoutSec 2 | Out-Null; $httpUp = $true; break } catch { Start-Sleep -Milliseconds 200 }
}
if (-not $httpUp) { Write-Host 'FATAL: mock HTTP 源未就绪'; Clean-All; exit 2 }

# ---- 本地 seeder（seed_main.exe：打印 SEED 行）----
$seedLog = Join-Path $root 'seed.log'
$seedExe = Join-Path $repo 'ffi\build\Release\seed_main.exe'
$seedProc = Start-Process -FilePath $seedExe -ArgumentList "$seedPort", (Join-Path $root 'seeddir') `
    -WorkingDirectory $root -RedirectStandardOutput $seedLog -RedirectStandardError (Join-Path $root 'seed.err') `
    -PassThru -WindowStyle Hidden
$magnet = $null
foreach ($i in 1..150) {
    if ($seedProc.HasExited) { break }
    Start-Sleep -Milliseconds 200
    $line = Get-Content $seedLog -Raw -ErrorAction SilentlyContinue
    if ($line -match 'SEED (magnet:\S+) PORT (\d+)') { $magnet = $Matches[1]; break }
}
if (-not $magnet) { Write-Host 'FATAL: seed_main 未输出 SEED 行'; Clean-All; exit 2 }

# ---- 启动 daemon（时间戳日志防 Tee 锁冲突；注入 libtorrent DLL 搜索路径）----
$exe = Join-Path $repo 'target\debug\smart-dl-daemon.exe'
$dlog = Join-Path $root ("daemon-{0}.log" -f $stamp)
$dllPath = (Join-Path $repo 'ffi\build\Release') + ';' + (Join-Path $repo 'ffi\vcpkg_installed\x64-windows\bin')
$envB = @{
    RUST_LOG                      = 'info'
    SMARTDL_MOCK_READY_DELAY_SECS = "$DelaySecs"
    SMART_DL_MOCK_URL             = "http://127.0.0.1:$httpPort/mockfile.bin"
    SMART_DL_MOCK_NAME            = 'mockfile.bin'
    SMART_DL_MOCK_SIZE            = (Get-Item $payload).Length
    SMARTDL_BT_PEER               = "127.0.0.1:$seedPort"
}
$envSave = @{}
foreach ($k in $envB.Keys) { $envSave[$k] = [Environment]::GetEnvironmentVariable($k); [Environment]::SetEnvironmentVariable($k, $envB[$k], 'Process') }
$savePath = [Environment]::GetEnvironmentVariable('PATH', 'Process')
[Environment]::SetEnvironmentVariable('PATH', "$dllPath;$savePath", 'Process')
$daemon = Start-Process -FilePath $exe -ArgumentList 'serve', '--config', (Join-Path $root 'config.toml') `
    -WorkingDirectory $root -RedirectStandardOutput $dlog -RedirectStandardError (Join-Path $root 'daemon.err') `
    -PassThru -WindowStyle Hidden
[Environment]::SetEnvironmentVariable('PATH', $savePath, 'Process')
foreach ($k in $envB.Keys) { [Environment]::SetEnvironmentVariable($k, $envSave[$k], 'Process') }

# 等 API 就绪（进程秒退 → 立即失败并回显错误）
$base = "http://127.0.0.1:$apiPort"
$ready = $false
foreach ($i in 1..60) {
    if ($daemon.HasExited) {
        Write-Host ("FATAL: daemon 秒退 code={0}" -f $daemon.ExitCode)
        Get-Content (Join-Path $root 'daemon.err') -ErrorAction SilentlyContinue | Write-Host
        Clean-All; exit 2
    }
    try { Invoke-RestMethod "$base/config" -TimeoutSec 2 | Out-Null; $ready = $true; break } catch { Start-Sleep -Milliseconds 200 }
}
if (-not $ready) { Write-Host 'FATAL: daemon 未就绪'; Clean-All; exit 2 }
Write-Host "== daemon pid=$($daemon.Id) api=$base delay=${DelaySecs}s downlimit=${DownLimitKbS}KB/s tag=$Tag"

function Snap($id) { try { Invoke-RestMethod "$base/tasks/$id" -TimeoutSec 4 } catch { $null } }

# ---- 加 BT 任务 ----
$tid = (Invoke-RestMethod -Method Post -Uri "$base/tasks" -ContentType 'application/json' `
        -Body (@{ url = $magnet } | ConvertTo-Json) -TimeoutSec 10).task_id
Write-Host "== task=$tid"

# ---- 分支门控 ----
$s = $null
if ($Tag -eq 'A') {
    # add 后立即 pause 并立即发起 fallback：让兜底 gate 在 metadata 到达前读到进度≈0
    # （环回 seeder 不受限速，metadata 落地后亚秒级完成）。等待窗内 Finished alert
    # 因记录态为 Paused 被丢弃——验证该交汇不产生 WEDGE 且链路最终 Completed。
    Write-Host '== gate A: add 后立即 pause'
} else {
    foreach ($i in 1..900) {
        $s = Snap $tid
        if ($s -and $s.total -gt 0 -and $s.done -ge $s.total) { break }
        Start-Sleep -Milliseconds 200
    }
    if (-not $s -or $s.total -le 0 -or $s.done -lt $s.total) { Write-Host "FATAL: BT 未完成 done=$($s.done) total=$($s.total)"; Clean-All; exit 4 }
    Write-Host "== gate B: 完成 done=$($s.done)/$($s.total)，静置 800ms 让 alert 先行"
    Start-Sleep -Milliseconds 800
}
Invoke-RestMethod -Method Post -Uri "$base/tasks/$tid/pause" -TimeoutSec 20 | Out-Null
Write-Host '== paused'

# ---- 异步发起 fallback（协调器进入 poll_ready 长等待）----
$fb = Start-Job -ScriptBlock {
    param($u)
    try { (Invoke-RestMethod -Uri $u -Method Post -TimeoutSec 1500) | ConvertTo-Json -Compress }
    catch { "FBERR: $($_.Exception.Message)" }
} -ArgumentList "$base/tasks/$tid/fallback"
Write-Host '== fallback 已发起'

if ($Tag -eq 'A') {
    # gate 竞态自检：fallback 若因进度≥50% 被 409 拒绝，本轮作废（脚本退出码 5，需重跑）
    Start-Sleep -Milliseconds 900
    $fbNow = Receive-Job $fb -ErrorAction SilentlyContinue
    if ("$fbNow" -match '409|ManualOnly|50%') {
        Write-Host "FATAL: fallback 被拒（环回竞态输给 metadata）: $fbNow"; Clean-All; Remove-Job $fb -Force; exit 5
    }
    Write-Host '== fallback 已被接受（gate 进度 <50%）'
} else {
    $tCfg = (Measure-Command { Invoke-RestMethod "$base/config" -TimeoutSec 4 | Out-Null }).TotalMilliseconds
    $tLst = (Measure-Command { Invoke-RestMethod "$base/tasks" -TimeoutSec 4 | Out-Null }).TotalMilliseconds
    Write-Host ("== baseline config={0:F0}ms tasks={1:F0}ms" -f $tCfg, $tLst)
    "baseline config=$([int]$tCfg)ms tasks=$([int]$tLst)ms" | Set-Content (Join-Path $root 'probe.csv')
}
$tCfg0 = (Measure-Command { Invoke-RestMethod "$base/config" -TimeoutSec 4 | Out-Null }).TotalMilliseconds
"baseline config=$([int]$tCfg0)ms" | Add-Content (Join-Path $root 'probe.csv')

# ---- 冻结判定：交替探 /tasks 与 /config，连续 3 轮双超时(>4s)认定 WEDGE ----
$failStreak = 0; $wedged = $false; $rounds = 0
$maxRounds = [Math]::Max(90, $DelaySecs + 180)
while ($rounds -lt $maxRounds) {
    Start-Sleep -Seconds 2; $rounds++
    $script:fT = $false
    $t1 = Measure-Command { try { Invoke-RestMethod "$base/tasks" -TimeoutSec 4 | Out-Null } catch { $script:fT = $true } }
    $okT = -not $script:fT
    $script:fC = $false
    $t2 = Measure-Command { try { Invoke-RestMethod "$base/config" -TimeoutSec 4 | Out-Null } catch { $script:fC = $true } }
    $okC = -not $script:fC
    "{0} tasks={1:F0}ms(ok={2}) config={3:F0}ms(ok={4})" -f (Get-Date -Format HH:mm:ss), $t1.TotalMilliseconds, $okT, $t2.TotalMilliseconds, $okC |
        Add-Content (Join-Path $root 'probe.csv')
    if (-not $okT -and -not $okC) { $failStreak++ } else { $failStreak = 0 }
    if ($failStreak -ge 3) { $wedged = $true; Write-Host '== WEDGE 判定成立（连续 3 轮双端点超时）'; break }
    if ($fb.State -eq 'Completed' -and $okT -and $okC) { Write-Host '== fallback 已返回'; break }
}

# ---- 取证 ----
$forensic = Join-Path $root 'forensics.txt'
"tag=$Tag wedged=$wedged rounds=$rounds apiPort=$apiPort daemonPid=$($daemon.Id)" | Set-Content $forensic
try {
    $p = Get-Process -Id $daemon.Id -ErrorAction Stop
    "threads=" + $p.Threads.Count | Add-Content $forensic
    $p.Threads | Group-Object { "{0}/{1}" -f $_.ThreadState, $_.WaitReason } |
        Sort-Object Count -Descending |
        ForEach-Object { "{0,3} x {1}" -f $_.Count, $_.Name } | Add-Content $forensic
} catch { "forensics(thread): $_" | Add-Content $forensic }
# minidump 尝试（沙箱/权限拒绝则记录原因即可）
try {
    $dump = Join-Path $root ("daemon-{0}.dmp" -f $stamp)
    $od = Start-Process rundll32.exe -ArgumentList "C:\Windows\System32\comsvcs.dll, MiniDump $($daemon.Id) `"$dump`" full" `
        -PassThru -Wait -WindowStyle Hidden -ErrorAction Stop
    "minidump exit=$($od.ExitCode) exists=$(Test-Path $dump)" | Add-Content $forensic
} catch { "minidump FAILED: $_" | Add-Content $forensic }
# 最终任务快照
try { "final snap: " + (Snap $tid | ConvertTo-Json -Compress) | Add-Content $forensic } catch { "final snap error: $_" | Add-Content $forensic }
# watchdog 存活判定
'--- watchdog tail ---' | Add-Content $forensic
Select-String -Path $dlog -Pattern 'watchdog alive' | Select-Object -Last 3 | ForEach-Object { $_.Line } | Add-Content $forensic
'--- fallback job ---' | Add-Content $forensic
Receive-Job $fb | Add-Content $forensic
# 时间线关键帧（最后 60 条 bugb）
'--- timeline tail ---' | Add-Content $forensic
Select-String -Path $dlog -Pattern '\[bugb\]' | Select-Object -Last 60 | ForEach-Object { $_.Line } | Add-Content $forensic
Copy-Item $dlog (Join-Path $root 'daemon-final.log') -Force

Write-Host "== wedged=$wedged forensic=$forensic"
Clean-All
Remove-Job $fb -Force -ErrorAction SilentlyContinue
exit $(if ($wedged) { 3 } else { 0 })
