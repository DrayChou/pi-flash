#!/bin/bash
# rustfmt 墓碑（2026-10-20 起：本仓**不再跑 rustfmt**）
#
# 政策（替代 2026-10-18 的「只格式化点名文件」折中方案）：
#   本仓完全放弃 rustfmt。`cargo fmt`、`rustfmt <file>`、本脚本的写盘模式
#   一律不用；排版靠手写（本仓作者风：紧凑、长行、少折行）。编译器、测试、
#   reviewer 都不看排版，仓库也没有 rustfmt-clean 基线 —— 跑格式化零收益。
#
# 为什么从「受控入口」进一步退成「彻底不用」（历史：2026-10-18 三次洪泛、
# 2026-10-19 mod.rs 连洗子树、2026-10-19 洪泛混进功能提交 1f66410）：
#   1. 本仓 200+ .rs 绝大多数不是 rustfmt-clean。哪怕只点名一个文件，也会把
#      该文件里所有无关行重排 → 功能提交里混进大片纯排版 diff，review 失真。
#   2. `rustfmt mod.rs` 会递归洗整棵模块树（点名 2 个 mod.rs 实际脏了 33 个文件）。
#   3. `crates/app/src/i18n.rs` 的 TABLE 被 i18n_audit 按行解析，折行即 panic。
#   4. 收益恒为零、风险恒不为零 ⇒ 唯一可靠的做法是让这个动作不可达。
#
# 保留的唯一用途（只读，不写盘）：
#   scripts/fmt.sh --check <file.rs>...   诊断某文件离 rustfmt 输出还差多少
#   scripts/fmt.sh --check --all          全仓只读扫描（统计用，不是门禁）
#
# 想要统一风格时的唯一姿势：**人工决策 + 单独一个 `style:` 提交**，且先想清楚
# 是否真的值得（当前结论：不值得，见 docs/agents/engineering.md §格式化）。

set -euo pipefail
cd "$(dirname "$0")/.."

EDITION=2024

die() {
    echo "错误：$*" >&2
    exit 1
}

print_usage() {
    cat <<'EOF'
本仓不再跑 rustfmt（写盘格式化已下线）。

只读诊断（不写盘）：
  scripts/fmt.sh --check <file.rs>...   看点名文件离 rustfmt 输出差多少
  scripts/fmt.sh --check --all          全仓扫一遗（只出统计，用于「要不要做一次
                                        性基线」这类决策，不是门禁）

不接受写盘模式（文件列表 / 目录 / -p 包名 等）——
本仓没有 rustfmt-clean 基线，格式化只会制造无关 diff，见 docs/agents/engineering.md。
EOF
}

if [ $# -eq 0 ]; then
    print_usage
    die "本仓不再跑 rustfmt（无参数也不会做任何事）"
fi

check_only=0
all=0
files=()

for arg in "$@"; do
    case "$arg" in
        -h|--help) print_usage; exit 0 ;;
        --check) check_only=1 ;;
        --all) all=1 ;;
        -*)
            die "不认识的选项 $arg（本脚本只收 --check / --all 和 .rs 文件路径）"
            ;;
        *)
            [ -e "$arg" ] || die "文件不存在：$arg"
            [ -d "$arg" ] && die "$arg 是目录——请点名具体 .rs 文件（本脚本只做只读诊断）"
            case "$arg" in
                *.rs) ;;
                *) die "$arg 不是 .rs 文件" ;;
            esac
            files+=("$arg")
            ;;
    esac
done

# 只要不是纯 --check，就没有任何可用动作：本仓的写盘格式化已下线。
if [ "$check_only" -eq 0 ]; then
    print_usage
    die "本仓不再跑 rustfmt：写盘格式化已下线，不要用它给提交掺排版 diff"
fi

if [ "$all" -eq 1 ]; then
    [ "${#files[@]}" -eq 0 ] || die "--all 与点名文件不要混用（要么全仓，要么点名）"
    mapfile -t files < <(git ls-files '*.rs')
fi

if [ "${#files[@]}" -eq 0 ]; then
    print_usage
    die "--check 需要点名具体 .rs 文件，或加 --all"
fi

# ---- 只读诊断 ----
rc=0
bad=0
for f in "${files[@]}"; do
    if ! rustfmt --edition "$EDITION" --check "$f" >/dev/null 2>&1; then
        bad=$((bad + 1))
        # 全仓模式只出统计（逐个刷 200 多行没意义）；点名模式列出文件名
        if [ "$all" -eq 0 ]; then
            echo "与 rustfmt 输出不一致：$f"
        fi
        rc=1
    fi
done

if [ "$all" -eq 1 ]; then
    echo "全仓扫描：${#files[@]} 个 .rs，其中 $bad 个不符合 rustfmt 输出。"
    echo "（注：crates/app/src/i18n.rs 永远计入不一致——TABLE 表行必须单行，见 engineering.md）"
else
    [ "$rc" -eq 0 ] && echo "全部符合 rustfmt 输出（注意：本仓并不要求这一点）。"
fi
echo "提示：本仓不跑 rustfmt；以上差异仅供人工判断，勿据此批量改写。"
exit "$rc"
