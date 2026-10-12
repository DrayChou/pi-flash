#!/usr/bin/env bash
# pi-flash 发版收尾一条龙：release.sh 八步全绿之后跑这个，替代手动尾部块
# （RELEASE.md §3.2 的等价手动序列保留作排查/降级路径）。
#
# 用法：
#   scripts/ship.sh <版本号> <说明文件.md>
#   例：scripts/ship.sh 0.2.0 tmp/release-notes-0.2.0.md
#
# 步骤：前置校验（版本三处一致、资产三件在位、tag 未占用）→ 发版提交
# （pathspec 限定六文件，不卷并行改动）→ tag → 推 main + tags → gh release
# create 挂三资产（紧跟推送，压缩薄壳用户的 404 竞态窗口，080 §4）。
# npm 薄壳不在此发：publish-npm.yml 随 tag 自动 OIDC 发布（RELEASE.md §4）。

set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=${1:-}
NOTES_FILE=${2:-}
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "用法: scripts/ship.sh <x.y.z> <说明文件.md>"; exit 1; }
[[ -f "$NOTES_FILE" ]] || { echo "说明文件不存在: $NOTES_FILE"; exit 1; }
ZIP_NAME="pi-flash-${VERSION}-win32-x64.zip"

# --- 前置校验 ---------------------------------------------------------------
grep -q "^version = \"${VERSION}\"" crates/app/Cargo.toml \
  || { echo "✗ crates/app/Cargo.toml 版本不是 ${VERSION}（先跑 scripts/release.sh ${VERSION} ${NOTES_FILE}）"; exit 1; }
grep -q "\"version\": \"${VERSION}\"" npm/package.json \
  || { echo "✗ npm/package.json 版本不是 ${VERSION}"; exit 1; }
for f in "dist/${ZIP_NAME}" "dist/${ZIP_NAME}.sha256" dist/SHA256SUMS; do
  [[ -f "$f" ]] || { echo "✗ 缺资产 dist/$f（先跑 scripts/release.sh）"; exit 1; }
done
git rev-parse -q --verify "refs/tags/v${VERSION}" >/dev/null \
  && { echo "✗ tag v${VERSION} 已存在——版本号永不复用（RELEASE.md §0）；若指向旧提交是测试残留，处置见 RELEASE.md §3.2"; exit 1; } || true

echo "==> 1/3 发版提交 + tag"
FILES=(crates/app/Cargo.toml npm/package.json Cargo.lock docs/CHANGELOG.md docs/CHANGELOG_EN.md)
git add "${FILES[@]}"
git commit -m "release v${VERSION}" -- "${FILES[@]}"
git tag "v${VERSION}"

echo "==> 2/3 推送 main + tags（tag 不在远端 gh --verify-tag 会拒；网络断则恢复后重推，勿强推）"
git push || { echo "✗ git push 失败：本地提交与 tag 不丢，网络恢复后 'git push && git push --tags'，再手动挂 Release（RELEASE.md §3.2）"; exit 1; }
git push --tags || { echo "✗ git push --tags 失败：恢复后重推 tags，再手动 gh release create（RELEASE.md §3.2）"; exit 1; }

echo "==> 3/3 挂 GitHub Release + 三资产"
gh release create "v${VERSION}" --verify-tag --title "v${VERSION}" \
  --notes-file "${NOTES_FILE}" \
  "dist/${ZIP_NAME}" "dist/${ZIP_NAME}.sha256" "dist/SHA256SUMS" \
  || { echo "✗ gh release create 失败：撞名多半是 release 已存在 → gh release upload v${VERSION} <资产> --clobber（RELEASE.md §6）"; exit 1; }

cat <<EOF
✓ v${VERSION} 已发布（npm 薄壳由 publish-npm.yml 自动发，分钟级）
验收（RELEASE.md §5）：
  gh release view v${VERSION}                          # 三资产 + 双语正文
  npm view pi-flash version --registry=https://registry.npmjs.org/   # CI 绿后 1-2 分钟内就绪，稍等再核
  真机：npm install -g pi-flash@latest --registry=https://registry.npmjs.org/
EOF
