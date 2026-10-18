#!/bin/bash
# 格式化唯一入口（AGENTS.md 铁律「禁整包 cargo fmt」的物理落点）
#
# 为什么需要这个脚本：`cargo fmt -p app` 的语义是**洗整个 package**，本仓
# 200+ .rs 绝大多数不是 rustfmt-clean（紧凑手写风），一跑就是几百文件洪泛；
# 且 crates/app/src/i18n.rs 的 TABLE 被 i18n_audit 按行解析，折行直接 panic。
# 2026-10-18 同一坑踩了三次，故把「做格式」收敛为唯一受控入口：
# **只能点名具体文件**，整包必须显式 --all + 二次确认。
#
# 用法：
#   scripts/fmt.sh <file.rs>...          只格式化点名的文件（默认、唯一安全姿势）
#   scripts/fmt.sh --check <file.rs>...  只检查不写盘（自检/CI 用）
#   scripts/fmt.sh --all                全仓专项（要求工作区干净 + 输入 yes）
#
# 脚本内置、绕不过去的铁律：
#   1. 无参数 = 报错退出。不存在「默认整包」这条默认值。
#   2. 传目录、包名、非 .rs、未跟踪文件 → 报错退出。
#   3. i18n.rs 永远跳过：TABLE 一行一条是 i18n_audit 的解析契约。
#   4. 跑完打印脏文件计数前后对比（哨兵）；一次涨 ≥20 个就提示疑似洪泛。
#
# 事后救火：scripts/fmt-flood-clean.sh（把 .rs 洪泛封进 stash，可恢复）

set -euo pipefail
cd "$(dirname "$0")/.."

# i18n_audit 按行解析表行，rustfmt 会把超宽行折成多行 → 永远跳过本文件。
SKIP_FILES=("crates/app/src/i18n.rs")
EDITION=2024

die() {
    echo "错误：$*" >&2
    exit 1
}

is_skipped() {
    local f
    for f in "${SKIP_FILES[@]}"; do
        [ "$f" = "$1" ] && return 0
    done
    return 1
}

# 哨兵用：全部脏条目（含未跟踪新文件）
dirty_count() {
    git status --short | wc -l | tr -d ' '
}

# --all 门禁用：只看**已跟踪**文件的改动——未跟踪的新文件不会混进格式化
# 结果（git ls-files '*.rs' 本就不含未跟踪），不该挡住专项动作。
tracked_dirty() {
    git status --porcelain | grep -v '^??' | wc -l | tr -d ' '
}

print_usage() {
    cat <<'EOF'
用法：
  scripts/fmt.sh <file.rs>...          只格式化点名的文件（推荐）
  scripts/fmt.sh --check <file.rs>...  只检查不写盘
  scripts/fmt.sh --all                 全仓专项格式化（需输入 yes 确认）

不接受目录或包名——整包格式化只能走 --all（会提示单独提交）。
EOF
}

if [ $# -eq 0 ]; then
    print_usage
    die "必须点名具体 .rs 文件（本仓禁止无差别格式化）"
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
            # 拦下 -p / -q 之类 cargo fmt 惯用参数：它们容易让人误以为在收窄范围
            die "不认识的选项 $arg（本脚本只收文件路径、--check、--all）"
            ;;
        *)
            [ -e "$arg" ] || die "文件不存在：$arg"
            [ -d "$arg" ] && die "$arg 是目录——请点名具体 .rs 文件，或整包跑 scripts/fmt.sh --all"
            case "$arg" in
                *.rs) ;;
                *) die "$arg 不是 .rs 文件" ;;
            esac
            git ls-files --error-unmatch "$arg" >/dev/null 2>&1 \
                || die "$arg 不在 git 索引里（本脚本只格式化已跟踪文件）"
            files+=("$arg")
            ;;
    esac
done

if [ "${#files[@]}" -eq 0 ] && [ "$all" -eq 0 ]; then
    print_usage
    die "没有给出任何文件"
fi

# ---- --all：全仓专项 ----
if [ "$all" -eq 1 ]; then
    echo "全仓格式化（专项）：仅用于「统一风格」这类独立提交。"
    echo "  · 会在工作区产生大量纯排版 diff"
    echo "  · i18n.rs 会被跳过（TABLE 表行必须保持单行，否则 i18n_audit 失败）"
    echo "  · 结果请**单独提交**（例：style: 全仓 rustfmt），不要混进功能提交"
    echo "  · 当前已跟踪文件的未提交改动：$(tracked_dirty) 个"
    [ "$(tracked_dirty)" -eq 0 ] \
        || die "工作区有未提交改动，全仓格式化会把它们混在一起——请先提交或 stash"
    read -r -p "确认执行全仓格式化？输入 yes 继续：" yn
    [ "$yn" = "yes" ] || { echo "已取消。"; exit 0; }
    mapfile -t all_files < <(git ls-files '*.rs')
    filtered=()
    for f in "${all_files[@]}"; do
        is_skipped "$f" && continue
        filtered+=("$f")
    done
    files=("${filtered[@]}")
    echo "待格式化 ${#files[@]} 个文件（跳过 ${SKIP_FILES[*]}）"
fi

# ---- 执行 ----
before=$(dirty_count)
skipped=0
targets=()
for f in "${files[@]}"; do
    if is_skipped "$f"; then
        echo "跳过 $f（i18n.rs 的 TABLE 表行必须保持单行）"
        skipped=$((skipped + 1))
        continue
    fi
    targets+=("$f")
done

if [ "$check_only" -eq 1 ]; then
    rc=0
    for f in "${targets[@]}"; do
        if ! rustfmt --edition "$EDITION" --check "$f" >/dev/null 2>&1; then
            echo "需格式化：$f"
            rc=1
        fi
    done
    [ "$rc" -eq 0 ] && echo "全部已是 rustfmt 输出格式。"
    exit "$rc"
fi

for f in "${targets[@]}"; do
    rustfmt --edition "$EDITION" "$f"
done
if [ "$skipped" -gt 0 ]; then
    echo "已格式化 ${#targets[@]} 个文件（跳过 $skipped 个）"
else
    echo "已格式化 ${#targets[@]} 个文件"
fi

# ---- 哨兵：dirty 计数前后对比 ----
after=$(dirty_count)
echo "脏文件计数：$before → $after"
if [ "$after" -gt "$before" ] && [ "$((after - before))" -ge 20 ]; then
    echo ""
    echo "⚠  脏文件一次涨了 $((after - before)) 个，疑似格式洪泛。"
    echo "   处置：scripts/fmt-flood-clean.sh（封进 stash，可恢复）"
    echo "   或回滚：git checkout HEAD -- <路径>"
fi
