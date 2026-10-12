#!/usr/bin/env node
"use strict";
// postinstall 入口：npm install / npm rebuild / npm install -g pi-flash@latest 都会跑到这里。
// 也可手动执行：node bin/fetch-payload.js   （= npm rebuild pi-flash，强制走一遍下载校验）
const lib = require("./lib");

async function main() {
  await lib.ensure();
}

if (require.main === module) {
  main().catch((error) => {
    console.error(`\n✗ pi-flash 载荷获取失败：\n  ${error.message}`);
    console.error(
      "  重试：npm rebuild pi-flash（或重跑 npm install -g pi-flash@latest）\n" +
        "  换源：PI_FLASH_MIRROR=<base> 指定唯一源；PI_FLASH_MIRRORS=<逗号分隔> 追加候选\n" +
        `  手动下载：${lib.assetUrls(lib.platformTag()).releasePage}`,
    );
    process.exit(1);
  });
}

module.exports = { main };
