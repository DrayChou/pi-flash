#!/bin/bash
# fmt 洪泛救火（pi-flash engineering.md §格式化 配套；2026-10-20 起 rustfmt
# 已彻底弃用，本脚本仍留作手滑后的事故回收）
#
# 场景：某会话对全仓跑了整包 cargo fmt，工作区出现大量纯格式化 .rs 改动。
# 本脚本把所有 .rs 改动封存进 stash（可恢复），docs 等非 .rs 改动留在工作区。
#
# 用法：scripts/fmt-flood-clean.sh
# 恢复：git stash pop（取回最近一次封存）
# 清理：git stash drop stash@{0}（确认永不要后销毁）

set -euo pipefail
cd "$(dirname "$0")/.."

# 工作区 .rs 改动清单（modified/untracked 都算；untracked 不入 stash 由 git 自动处理）
mapfile -t files < <(git status --short | grep '\.rs$' | awk '{print $2}')

if [ "${#files[@]}" -eq 0 ]; then
    echo "工作区没有 .rs 改动，无需处理。"
    exit 0
fi

echo "发现 ${#files[@]} 个 .rs 改动文件，抽样 diff（确认是否纯格式化）："
echo "---"
git diff --stat -- "${files[@]:0:5}" | tail -5
echo "---"

# -w 后无实质变化的文件数（纯空白/折行的特征）
pure=0
for f in "${files[@]}"; do
    if git diff --quiet -w -- "$f" 2>/dev/null; then
        pure=$((pure + 1))
    fi
done
echo "其中 ${pure}/${#files[@]} 个文件忽略空白后无变化（纯格式化特征）。"

if [ "${#files[@]}" -lt 10 ]; then
    echo "改动文件少于 10 个，不像洪泛——请人工确认后自行处理。"
    exit 1
fi

read -p "封存这 ${#files[@]} 个文件到 stash？(y/N) " yn
if [ "$yn" = "y" ]; then
    git stash push -m "fmt-flood backup $(date +%m-%d %H:%M)" -- "${files[@]}"
    echo ""
    echo "已封存。工作区剩余改动："
    git status --short
    echo ""
    echo "恢复：git stash pop ｜ 永久销毁：git stash drop stash@{0}"
fi
