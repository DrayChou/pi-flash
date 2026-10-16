#!/usr/bin/env bash
# 081 真机更新链路端到端（设计：docs/模块设计/081-软件自动更新.md §7）
#
# 验什么：应用内更新全链路——检测 → 真下载 → 真 SHA-256 → 真解压（1.5 万文件）
#   → 落位新载荷目录 → UI 提示 → 点击重启 / 直启旧 exe 交接 → 新版本进程在跑
#   且不再重下 → 更旧版本目录被清理。全程隔离目录 + pif-ui 自动化：不抢真实
#   屏幕/鼠标/键盘，不碰真实安装目录。
#
# 怎么造"旧版 → 新版"（**不改任何版本号、不碰 Cargo.toml / Cargo.lock**）：
#   - 旧版载荷 = 当前源码构建的 exe（真实版本 V）+ 真 release 载荷的 node.exe/vendor
#   - 新版 zip  = dist/ 里最新的真 release 资产，**只把文件名与新版本号改成 V+1**
#     （内容仍是真资产）→ 下载/哈希/解压全是真的；假 API 只说 latest = V+1
#   - 重启后把假 API 的 latest 改回 V：新进程看到"已是最新" → 断言 state=idle、
#     不再重下（即用户报的"重启后版本没更新→再次下载"闭环已断）
#   注意：新版目录里的 exe 其实是 V 版编译产物（改名而已），所以它启动时会打印
#   一行「版本戳与进程版本不符，跳过启动交接」——这是本套夹具的预期产物。
#
# 用法：
#   scripts/upd-e2e.sh [--new-version 0.1.11] [--port 18765] [--keep]
# 退出码 0 = 全绿。
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT_W=$(pwd -W)                     # D:/ai_workspace/pi-flash（Windows 侧路径，给 app 用）
T=tmp/upd-e2e
PUI=target/debug/pif-ui.exe
APP=target/debug/pi-flash.exe
T=tmp/upd-e2e
T_WIN=${T//\//\\}                 # tmp\upd-e2e（PowerShell 眼里的相对路径）
PORT=18765
NEW_VER=""
KEEP=0

while [ $# -gt 0 ]; do
  case "$1" in
    --new-version) NEW_VER=$2; shift 2 ;;
    --port) PORT=$2; shift 2 ;;
    --keep) KEEP=1; shift ;;
    *) echo "未知参数：$1"; exit 2 ;;
  esac
done

pass=0; fail=0
ok()   { pass=$((pass+1)); echo "  ✓ $1"; }
bad()  { fail=$((fail+1)); echo "  ✗ $1${2:+  —— $2}"; }
step() { echo -e "\n$1"; }

# 测试实例的 pid（只认本夹具目录下的进程，绝不碰用户真实实例）
test_pids() {
  powershell.exe -NoProfile -Command \
    "Get-CimInstance Win32_Process -Filter \"Name='pi-flash.exe'\" | Where-Object { \$_.ExecutablePath -like '*${ROOT_W//\//\\}\\$T_WIN*' } | Select-Object -ExpandProperty ProcessId" \
    2>/dev/null | tr -d '\r' | grep -E '^[0-9]+$' || true
}
kill_test_instances() {
  for pid in $(test_pids); do
    powershell.exe -NoProfile -Command "Stop-Process -Id $pid -Force" 2>/dev/null
  done
  sleep 1
}
# 当前在跑的 pi-flash 进程 exe 路径（Windows 形式）
running_paths() {
  powershell.exe -NoProfile -Command \
    "Get-CimInstance Win32_Process -Filter \"Name='pi-flash.exe'\" | Select-Object -ExpandProperty ExecutablePath" \
    2>/dev/null | tr -d '\r' | grep -v '^$' || true
}
wait_for() { # wait_for <秒> <命令...>：命令返回 0 即成功
  local deadline=$((SECONDS+$1)); shift
  while [ $SECONDS -lt $deadline ]; do
    if "$@" >/dev/null 2>&1; then return 0; fi
    sleep 2
  done
  return 1
}
# 进程路径子串匹配（大小写无关）。不用 grep：本机 MSYS grep 3.0 的
# `-i -F --` 组合会 core dump（SIGABRT，见 docs/agents/engineering.md），
# 且轮询里 case 不长进程。
has_proc() {
  local line
  while IFS= read -r line; do
    case "${line,,}" in *"${1,,}"*) return 0 ;; esac
  done < <(running_paths)
  return 1
}
no_proc() { ! has_proc "$1"; }

# 退出必清：测试实例 + 占着端口的假源（幂等；绝不碰用户真实实例）
kill_port_server() {
  powershell.exe -NoProfile -Command "Get-CimInstance Win32_Process -Filter \"Name='python.exe'\" | Where-Object { \$_.CommandLine -like '*$PORT*' } | ForEach-Object { Stop-Process -Id \$_.ProcessId -Force }" 2>/dev/null
}
cleanup_on_exit() {
  kill_test_instances
  kill_port_server
}
trap cleanup_on_exit EXIT

# --- 前置 ---------------------------------------------------------------
[ -x "$PUI" ] || { echo "缺 $PUI —— 先跑 cargo build"; exit 2; }
[ -x "$APP" ] || { echo "缺 $APP —— 先跑 cargo build"; exit 2; }
ZIP=$(ls -1 dist/pi-flash-*-win32-x64.zip 2>/dev/null | while read -r f; do
        v=$(basename "$f" | sed -E 's/^pi-flash-(.*)-win32-x64\.zip$/\1/'); echo "$v $f"; done \
      | sort -t. -k1,1n -k2,2n -k3,3n | tail -1)
[ -n "$ZIP" ] || { echo "dist/ 里没有 pi-flash-*-win32-x64.zip（先跑 scripts/release.sh）"; exit 2; }
REAL_VER=${ZIP%% *}; ZIP=${ZIP#* }
if [ -z "$NEW_VER" ]; then
  IFS=. read -r maj min pat <<<"$REAL_VER"; NEW_VER="$maj.$min.$((pat+1))"
fi
echo "真资产：$ZIP（版本 $REAL_VER）→ 夹具把新版本装作 v$NEW_VER，端口 $PORT"

# --- 装配 ---------------------------------------------------------------
kill_test_instances
kill_port_server
rm -rf "$T"
mkdir -p "$T/pkg/payload" "$T/site/v$NEW_VER" "$T/site/releases/tags" "$T/home"
# 载荷骨架：真 release 载荷（node.exe + vendor，硬链接 = 秒级、不复制 300MB）
cp -al dist/pi-flash/. "$T/pkg/payload/" 2>/dev/null || cp -a dist/pi-flash/. "$T/pkg/payload/"
rm -f "$T/pkg/payload/pi-flash.exe"                     # 先断硬链接再覆盖，绝不改到 dist/
cp "$APP" "$T/pkg/payload/pi-flash.exe"
rm -f "$T/pkg/payload/.payload-version"
printf '%s\n' "$REAL_VER" > "$T/pkg/payload/.payload-version"
# 更旧版本的完整载荷目录（验启动清理，不碰活目录）。假 exe 即可：清理只判存在性。
# 0.0.9 永远比任何现实版本旧，不会与实际版本撞车
STALE_VER=0.0.9
mkdir -p "$T/pkg/payload-$STALE_VER"
printf 'x' > "$T/pkg/payload-$STALE_VER/pi-flash.exe"
printf '%s\n' "$STALE_VER" > "$T/pkg/payload-$STALE_VER/.payload-version"

# 假 GitHub Release 站：真 zip 改名 + 真 sha256 + 假 API
cp "$ZIP" "$T/site/v$NEW_VER/pi-flash-$NEW_VER-win32-x64.zip"
( cd "$T/site/v$NEW_VER" && sha256sum "pi-flash-$NEW_VER-win32-x64.zip" > "pi-flash-$NEW_VER-win32-x64.zip.sha256" \
  && cp "pi-flash-$NEW_VER-win32-x64.zip.sha256" SHA256SUMS )
release_json() { printf '{"tag_name":"v%s","body":"# v%s 测试更新日志\\n\\n- 更新落位改版（%s）\\n"}' "$1" "$1" "$(date +%F)"; }
release_json "$NEW_VER" > "$T/site/releases/latest"
cp "$T/site/releases/latest" "$T/site/releases/tags/v$NEW_VER"
release_json "$REAL_VER" > "$T/site/releases/tags/v$REAL_VER"   # 新进程首启读更新日志
# 新版本首启自动开更新日志页：last_seen 在阶段②重启前才预置成更旧版本——
# 现在不写，免得 shim 自己也开更新日志页（那样截图就拍不到版本号位的更新提示了）

# --- 起假源 + 起 app ----------------------------------------------------
( nohup python -m http.server "$PORT" --bind 127.0.0.1 --directory "$T/site" > "$T/http.log" 2>&1 & )
sleep 2
curl -sf "http://127.0.0.1:$PORT/releases/latest" >/dev/null && ok "假源已就绪（127.0.0.1:$PORT）" || { bad "假源起不来" "$(cat "$T/http.log")"; exit 1; }

export PI_FLASH_DIR="$ROOT_W/$T/home"
export PI_FLASH_AUTOMATION=auto
launch_shim() {
  PI_FLASH_UPDATE_CHECK=1 \
  PI_FLASH_MIRROR="http://127.0.0.1:$PORT" \
  PI_FLASH_RELEASES_API="http://127.0.0.1:$PORT/releases" \
  nohup "$T/pkg/payload/pi-flash.exe" >> "$T/app.log" 2>&1 &
}
: > "$T/app.log"
launch_shim
booted=0
for _ in $(seq 1 30); do                     # pif-ui wait 要求实例已登记，先轮询
  if $PUI wait --path app.booted --truthy --timeout 5 >/dev/null 2>&1; then booted=1; break; fi
  sleep 2
done
[ "$booted" = 1 ] || { bad "app 起不来" "$(tail -20 "$T/app.log")"; exit 1; }
ok "旧版载荷（v$REAL_VER 源码构建）已启动"

# --- 阶段① 检测 → 下载 → 落位 ------------------------------------------
step "① 检测 + 真下载 + 真校验 + 真解压 → 落位新载荷目录"
$PUI wait --path app.update.state --eq ready --timeout 900 >/dev/null 2>&1 \
  && ok "进入 ready（下载/解压完成）" || bad "没进 ready（timeout）"
NEWDIR="$T/pkg/payload-$NEW_VER"
if [ "$(cat "$NEWDIR/.payload-version" 2>/dev/null | tr -d '\r\n')" = "$NEW_VER" ]; then
  ok "新载荷目录 $NEWDIR 落位，版本戳 = $NEW_VER"
else
  bad "新载荷目录/版本戳不对" "$(ls -d "$T"/pkg/payload-* 2>/dev/null | tr '\n' ' ')"
fi
[ -d "$NEWDIR/vendor/pi" ] && ok "新载荷内容完整（vendor/pi 在位）" || bad "新载荷内容不完整"
[ "$(cat "$T/pkg/payload/.payload-version" | tr -d '\r\n')" = "$REAL_VER" ] \
  && ok "旧载荷目录未被碰（仍 v$REAL_VER）" || bad "旧载荷目录被改动了"
[ -z "$(ls -d "$T"/pkg/update.tmp-* 2>/dev/null)" ] && ok "无 update.tmp-* 半截残留" || bad "有 update.tmp-* 残留"
[ ! -d "$T/pkg/payload-$STALE_VER" ] && ok "更旧版本目录（payload-$STALE_VER）已被启动清理删掉" || bad "更旧版本目录没被清理"
snap=$($PUI snapshot app --only update 2>/dev/null)
echo "$snap" | grep -q "\"target\": \"$NEW_VER\"" && ok "UI 状态 target = $NEW_VER" || bad "UI target 不对" "$snap"
if $PUI shot "$ROOT_W/$T/ready.png" >/dev/null 2>&1; then ok "证据截图：$T/ready.png"; else bad "截图失败"; fi

# --- 阶段② 点击重启 -----------------------------------------------------
step "② 点「下载完成，点击重启更新」→ 新版本进程跑在新目录，且不再重下"
release_json "$REAL_VER" > "$T/site/releases/latest"          # 翻回真版本 = 新进程看到"已是最新"
printf '{"last_seen_version":"0.0.1"}' > "$T/home/update-state.json"   # 让新进程首启开更新日志
$PUI exec update.restart >/dev/null 2>&1
wait_for 90 has_proc "payload-$NEW_VER" && ok "新进程 = payload-$NEW_VER/pi-flash.exe" || bad "新进程没起来" "$(running_paths | tr '\n' ' ')"
wait_for 60 no_proc "$T_WIN\\pkg\\payload\\pi-flash.exe" && ok "旧（shim）进程已退出" || bad "shim 进程还活着"
$PUI clean >/dev/null 2>&1
$PUI wait --path app.booted --truthy --timeout 60 >/dev/null 2>&1 && ok "新实例自动化已就绪" || bad "新实例自动化连不上"
ver=$($PUI snapshot app --only version 2>/dev/null)
upd=$($PUI snapshot app --only update 2>/dev/null)
echo "$ver" | grep -q "\"version\": \"$REAL_VER\"" && ok "运行版本 = $REAL_VER" || bad "运行版本不对" "$ver"
echo "$upd" | grep -q '"state": "idle"' && ok "update.state = idle（不再提示/不再重下）" || bad "state 不是 idle" "$upd"
echo "$upd" | grep -q "\"changelog_open\": \"$REAL_VER\"" && ok "新版本首启自动打开更新日志页" || bad "更新日志页没自动打开" "$upd"
[ -z "$(ls -d "$T"/pkg/payload-* 2>/dev/null | grep -v "payload-$NEW_VER")" ] \
  && ok "磁盘上只剩一个版本化载荷目录（无重下产物）" || bad "出现多余的载荷目录（疑似重下）" "$(ls -d "$T"/pkg/payload-* | tr '\n' ' ')"

# --- 阶段③ 直启旧 exe 交接（双击路径）-----------------------------------
step "③ 双击路径：直接启动旧 payload/pi-flash.exe → 启动交接进新版"
$PUI exec app.quit >/dev/null 2>&1
wait_for 30 no_proc "payload-$NEW_VER" >/dev/null 2>&1
sleep 2
launch_shim
wait_for 60 has_proc "payload-$NEW_VER" && ok "旧 exe 直启后运行的是 payload-$NEW_VER" || bad "交接失败" "$(running_paths | tr '\n' ' ')"
grep -q "交接给已就位的新载荷 v$NEW_VER" "$T/app.log" && ok "日志有交接记录" || bad "日志无交接记录"

# --- 收尾 ---------------------------------------------------------------
kill_test_instances
kill_port_server
if [ "$KEEP" = 1 ]; then
  echo "（--keep：现场全留）"
else
  rm -rf "$T/pkg" "$T/site" "$T/home"      # 载荷载荷/假源是大头，证据（logs/png）保留
fi
echo
echo "=================================================="
echo "  通过 $pass 项，失败 $fail 项"
echo "  证据/现场：$T"
echo "=================================================="
[ "$fail" = 0 ] || exit 1
