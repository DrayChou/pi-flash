#!/bin/bash
# agent-workspace.sh —— 多 agent 并行工作区：一键本地克隆独立工作副本（不是 worktree）
#
# 为什么需要这个脚本：多个 coding agent 挤在同一个工作目录里会互相等——
# cargo 对 target/ 有文件锁，谁的构建先跑另一个就得排队；git index.lock 与
# 半成品改动互相污染，提交也得让路。本仓的可执行解：每个 agent 一个本地
# 克隆（同级目录），target/、pi-flash-dev/、git 索引全部按克隆天然隔离，
# 构建与提交从此互不阻塞。
#
# 用法（在黄金仓库或任一克隆里执行）：
#   scripts/agent-workspace.sh <名字>              # 克隆到同级 ../pi-flash-<名字>
#   scripts/agent-workspace.sh <名字> --target     # 另复制 target/ 热缓存，省新克隆首次全量编译
#
# 为什么便宜：
#   - git 本地克隆自动硬链接 .git 对象（同盘）：秒级完成、几乎不占额外磁盘
#   - vendor/pi 只入库 3 个文件，克隆后 npm ci 一次即可（复用全局 npm 缓存）
#   - dev 数据落在各克隆自己的 pi-flash-dev/（paths.rs 按编译期 workspace 根定位）
#   - vendored pi 运行时按 exe 相对路径定位（vendor.rs），各克隆用各自的 vendor/pi
#
# 铁律：
#   1. 不用 git worktree。一个克隆目录一个 agent，各自完整、互不纠缠。
#   2. 不要把 A 克隆编出的 exe 拷到 B 克隆里跑：dev 数据与 vendored pi 的
#      定位会跟着编译期/exe 路径走，拷 exe 不拷仓库必然串。
#   3. push 前 git pull --rebase。各克隆都直接推 origin；任务层面靠 bd
#      领取保证文件不相交，rebase 就基本无冲突。
#
# 已知共享面：~/.pi/agent 是 pi 自己的会话存储，两个克隆的应用实例同时跑
# 会共用它（构建/测试不受影响；确需实例级隔离另行处理）。

set -euo pipefail
cd "$(dirname "$0")/.."

die() {
    echo "错误：$*" >&2
    exit 1
}

print_usage() {
    cat <<'EOF'
用法：
  scripts/agent-workspace.sh <名字>            克隆到同级 ../pi-flash-<名字>
  scripts/agent-workspace.sh <名字> --target   另复制 target/ 热缓存（robocopy /MT）

名字建议用 w1/w2/…；克隆完成后在目标目录里启动该 agent 的会话即可。
EOF
}

[ $# -eq 0 ] && { print_usage; die "必须给出工作区名字（如 w1）"; }

name="${1:?}"
with_target=0
for arg in "${@:2}"; do
    case "$arg" in
        --target) with_target=1 ;;
        -h|--help) print_usage; exit 0 ;;
        *) die "不认识的选项 $arg（只收 <名字> 和 --target）" ;;
    esac
done

case "$name" in
    -*|*/*|*.*) die "名字只能是简单词（如 w1），收到：$name" ;;
esac

src="$(git rev-parse --show-toplevel)"
dst="$(dirname "$src")/pi-flash-$name"
[ -e "$dst" ] && die "目标已存在：$dst"

echo "==> 本地克隆（.git 对象硬链接，秒级）"
echo "    $src → $dst"
git clone --quiet "$src" "$dst"

echo "==> 对齐 origin/main（尽力而为）"
if ! git -C "$dst" pull --rebase --quiet; then
    echo "    警告：未能对齐 origin/main，开工前请手动 git pull --rebase" >&2
fi

echo "==> vendor/pi: npm ci"
(cd "$dst/vendor/pi" && npm ci --no-fund --no-audit) \
    || die "npm ci 失败：$dst/vendor/pi"

if [ "$with_target" -eq 1 ]; then
    echo "==> 复制 target/ 热缓存（省首次全量编译；未命中等同全量，无副作用）"
    if command -v robocopy >/dev/null 2>&1; then
        # robocopy 退出码 0-7 都是成功（位标志），set -e 下先 || 接住再判
        rc=0
        robocopy "$src/target" "$dst/target" //E //MT:32 //NFL //NDL //NJH //NP || rc=$?
        [ "$rc" -lt 8 ] || echo "警告：robocopy 失败（rc=$rc），新克隆将走全量编译" >&2
    else
        cp -a "$src/target" "$dst/target"
    fi
fi

echo "==> 完成：$dst"
echo "    cd \"$dst\" 后在该目录启动 agent 会话；构建/测试/dev 数据均隔离在本克隆内"
echo "    再开一个并行工作区：scripts/agent-workspace.sh w2"
