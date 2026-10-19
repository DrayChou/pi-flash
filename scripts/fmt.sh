#!/bin/bash
# 格式化入口（2026-10-20 起：本仓已是 rustfmt-clean 仓，格式化是机械动作）
#
# 常规情况下**不需要跑本脚本**：`.githooks/pre-commit` 会对暂存的 `crates/**.rs`
# 自动跑 rustfmt 并重新暂存（agent / 人都不必记排版规则）。本脚本是给
# 「想主动、批量或只读检查」的场景用的。
#
# 用法：
#   scripts/fmt.sh <file.rs>...          格式化点名文件（crates/** 下的）
#   scripts/fmt.sh --all                 格式化全部 crates/**.rs（需输 yes）
#   scripts/fmt.sh --check <file.rs>...  只检查不写盘
#   scripts/fmt.sh --check --all         只读全仓体检（统计用，不是门禁）
#
# 硬口径（写死，绕不过去）：
#   · `vendor/**` 永不格式化：vendored 上游副本（gpui / gpui-component，
#     workspace exclude），格式化会把本地补丁面炸开。
#   · `crates/app/src/i18n.rs` 的 TABLE 由文件内 `#[rustfmt::skip]` 局部豁免，
#     本脚本无需特判——但**不要删那个属性**（i18n_audit 按行解析表行，折行即炸）。
#   · 配置跟着仓库根的 rustfmt.toml（edition 2024）。

set -euo pipefail
cd "$(dirname "$0")/.."

EDITION=2024

die() {
    echo "错误：$*" >&2
    exit 1
}

print_usage() {
    cat <<'EOF'
用法：
  scripts/fmt.sh <file.rs>...          格式化点名文件（crates/** 下）
  scripts/fmt.sh --all                 格式化全部 crates/**.rs（需输 yes 确认）
  scripts/fmt.sh --check <file.rs>...  只检查不写盘
  scripts/fmt.sh --check --all         只读全仓体检（统计用）

平时不用跑：pre-commit 会自动格式化暂存的 crates/**.rs。
vendor/** 永不格式化（vendored 上游副本）。
EOF
}

check_only=0
all=0
files=()

for arg in "$@"; do
    case "$arg" in
        -h|--help) print_usage; exit 0 ;;
        --check) check_only=1 ;;
        --all) all=1 ;;
        -*)
            die "不认识的选项 $arg（只收文件路径、--check、--all）"
            ;;
        *)
            [ -e "$arg" ] || die "文件不存在：$arg"
            [ -d "$arg" ] && die "$arg 是目录——请点名具体 .rs 文件，或整包跑 scripts/fmt.sh --all"
            case "$arg" in
                *.rs) ;;
                *) die "$arg 不是 .rs 文件" ;;
            esac
            case "$arg" in
                vendor/*) die "$arg 在 vendor/ 下——vendored 上游副本永不格式化" ;;
            esac
            files+=("$arg")
            ;;
    esac
done

if [ "$all" -eq 1 ]; then
    [ "${#files[@]}" -eq 0 ] || die "--all 与点名文件不要混用（要么全仓，要么点名）"
    mapfile -t files < <(git ls-files 'crates/*.rs')
fi

if [ "${#files[@]}" -eq 0 ]; then
    print_usage
    die "没有给出任何文件"
fi

# ---- 只读检查 ----
if [ "$check_only" -eq 1 ]; then
    rc=0
    bad=0
    for f in "${files[@]}"; do
        if ! rustfmt --edition "$EDITION" --check "$f" >/dev/null 2>&1; then
            bad=$((bad + 1))
            [ "$all" -eq 0 ] && echo "与 rustfmt 输出不一致：$f"
            rc=1
        fi
    done
    if [ "$all" -eq 1 ]; then
        echo "全仓体检：${#files[@]} 个 .rs，其中 $bad 个不符合 rustfmt 输出。"
        [ "$bad" -eq 0 ] && echo "干净（基线保持完好）。"
    else
        [ "$rc" -eq 0 ] && echo "全部符合 rustfmt 输出。"
    fi
    exit "$rc"
fi

# ---- 写盘 ----
if [ "$all" -eq 1 ]; then
    echo "全仓格式化 crates/**.rs（${#files[@]} 个文件）。"
    echo "  · 本仓已有 rustfmt 基线，正常情况下这只会动你没格式化的新代码"
    echo "  · 结果请单独提交（例：style: 全仓 rustfmt）"
    read -r -p "确认执行？输入 yes 继续：" yn
    [ "$yn" = "yes" ] || { echo "已取消。"; exit 0; }
fi

before=$(git status --short | wc -l | tr -d ' ')
for f in "${files[@]}"; do
    rustfmt --edition "$EDITION" "$f"
done
echo "已格式化 ${#files[@]} 个文件"

after=$(git status --short | wc -l | tr -d ' ')
echo "脏文件计数：$before → $after"
if [ "$after" -gt "$before" ] && [ "$((after - before))" -ge 20 ]; then
    echo ""
    echo "⚠  脏文件一次涨了 $((after - before)) 个——基线可能已被破坏。"
    echo "   处置：git status 核对；scripts/fmt-flood-clean.sh 可封存到 stash"
fi
