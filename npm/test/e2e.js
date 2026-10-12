#!/usr/bin/env node
"use strict";
// pi-flash npm 薄壳端到端自测（本地回环：不访问 GitHub/npm registry，不启动 GUI）。
//
//   node npm/test/e2e.js            # 全量跑，结束打印 PASS/FAIL 汇总
//   node npm/test/e2e.js --keep     # 保留工作目录 tmp/e2e-auto 便于排查
//
// 覆盖 docs/模块设计/080-软件分发.md §5 P1 验收中可本地验证的项：
//   ① 薄壳安装（postinstall 下载+校验+落位）      ② stdio/退出码透传
//   ③ 删载荷自愈                                  ④ 版本戳不符自愈
//   ⑤ --ignore-scripts 首启自愈                   ⑥ SHA256SUMS 兜底
//   ⑦ 损坏载荷被 SHA-256 拦截                      ⑧ 旧→新版本更新=同一条命令
//   ⑨ 同版本快路径不重复下载
//   ⑩ 081 应用内更新落位：staging+标记 → 启动时 swap
//   ⑪ 载荷比薄壳新（应用内更新先行）→ 不降级、不重复下载
//
// 夹具：把 node 可执行文件冒充 pi-flash.exe（spawn 语义与真 exe 等价），
// 载荷 zip 复刻 release.sh 的 Compress-Archive 包装目录布局（根为 pi-flash/）。
// 仅支持 win32-x64 开发机（fixture 布局按 win32 设计）。

const { spawn, spawnSync } = require("child_process");
const http = require("http");
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");

const REPO = path.resolve(__dirname, "..", "..");
const WORK = path.join(REPO, "tmp", "e2e-auto");
const NPM_DIR = path.join(REPO, "npm");
const KEEP = process.argv.includes("--keep");

// 夹具版本刻意用 9.9.x 假版本，与 registry / GitHub Release 上的真实发版脱钩
// （曾经写死 0.1.1→0.1.2，真实 0.1.2 发布后看测试输出易混淆）。语义依赖：
// TEST_OLD < TEST_NEW（⑧ 更新、⑪ 不降级）；④ 的「更旧才修」戳用 9.9.0。
const TEST_OLD = "9.9.1";
const TEST_NEW = "9.9.2";

let failures = 0;
function check(name, cond, detail = "") {
  if (cond) console.log(`  ✓ ${name}`);
  else {
    failures += 1;
    console.log(`  ✗ ${name}${detail ? `  —— ${detail}` : ""}`);
  }
}
function fatal(msg) {
  console.error(`✗ ${msg}`);
  process.exit(1);
}

// 异步执行：本进程兼当本地下载服务器，sync 阻塞事件循环会让下载请求饿死（30s 超时）
function runAsync(cmd, args, opts = {}) {
  return new Promise((resolve) => {
    const child = spawn(cmd, args, { windowsHide: true, ...opts });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d) => (stdout += d));
    child.stderr.on("data", (d) => (stderr += d));
    child.on("error", (error) => resolve({ status: -1, stdout, stderr: stderr + error.message }));
    child.on("close", (status) => resolve({ status, stdout, stderr }));
  });
}
async function npm(args, env = {}) {
  // npm 是 .cmd，Node ≥18.20 对 .cmd 必须走 shell
  return runAsync(`npm ${args.join(" ")}`, [], {
    shell: true,
    env: { ...process.env, ...env },
  });
}
function nodeRun(script, args, env = {}) {
  return runAsync(process.execPath, [script, ...args], {
    env: { ...process.env, ...env },
  });
}
function sha256File(file) {
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

// --- 本地“GitHub Releases”静态站 ---------------------------------------------
function startServer(root, port) {
  const server = http.createServer((req, res) => {
    const file = path.join(root, decodeURIComponent(req.url.split("?")[0]));
    fs.readFile(file, (err, data) => {
      if (err) {
        res.statusCode = 404;
        return res.end("not found");
      }
      res.end(data);
    });
  });
  return new Promise((resolve, reject) => {
    server.listen(port, "127.0.0.1", () => resolve(server));
    server.on("error", reject);
  });
}

// --- 载荷夹具 -----------------------------------------------------------------
function buildPayload(siteDir, version) {
  const fixRoot = path.join(WORK, `fix-${version}`);
  const pkg = path.join(fixRoot, "pi-flash");
  fs.mkdirSync(pkg, { recursive: true });
  fs.copyFileSync(process.execPath, path.join(pkg, "pi-flash.exe")); // 冒充真 exe
  fs.writeFileSync(path.join(pkg, "marker.txt"), `payload-v${version}\n`);

  const verDir = path.join(siteDir, `v${version}`);
  fs.mkdirSync(verDir, { recursive: true });
  const zip = path.join(verDir, `pi-flash-${version}-win32-x64.zip`);
  // 复刻 release.sh：Compress-Archive -Path <fix>\pi-flash → zip 根带 pi-flash/ 包装目录
  const r = spawnSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-Command",
      `Compress-Archive -LiteralPath '${fixRoot}\\pi-flash' -DestinationPath '${zip}' -Force`,
    ],
    { encoding: "utf8", windowsHide: true },
  );
  if (r.status !== 0) fatal(`夹具压缩失败：${r.stderr}`);
  const hash = sha256File(zip);
  const name = path.basename(zip);
  fs.writeFileSync(path.join(verDir, `${name}.sha256`), `${hash}  ${name}\n`);
  fs.writeFileSync(path.join(verDir, "SHA256SUMS"), `${hash}  ${name}\n`);
  return zip;
}

function payloadDirOf(prefix) {
  return path.join(prefix, "node_modules", "pi-flash", "payload");
}
function stampOf(prefix) {
  try {
    return fs.readFileSync(path.join(payloadDirOf(prefix), ".payload-version"), "utf8").trim();
  } catch {
    return null;
  }
}
function launcherOf(prefix) {
  return path.join(prefix, "node_modules", "pi-flash", "bin", "pi-flash.js");
}

// --- 主流程 -------------------------------------------------------------------
async function main() {
  if (process.platform !== "win32") {
    console.log("本夹具按 win32-x64 布局设计，请在 Windows 开发机上运行。");
    process.exit(0);
  }
  if (fs.existsSync(WORK)) fs.rmSync(WORK, { recursive: true, force: true });
  fs.mkdirSync(WORK, { recursive: true });

  const site = path.join(WORK, "site");
  console.log("构建夹具载荷…");
  buildPayload(site, TEST_OLD);
  buildPayload(site, TEST_NEW);

  const PORT = 18000 + (process.pid % 900);
  const siteSrv = await startServer(site, PORT);
  const mirror = `http://127.0.0.1:${PORT}`;
  // 损坏站：复制 TEST_OLD 后往 zip 尾部追加字节（hash 失效）
  const badSite = path.join(WORK, "site-bad");
  fs.cpSync(site, badSite, { recursive: true });
  const badZip = path.join(badSite, `v${TEST_OLD}`, `pi-flash-${TEST_OLD}-win32-x64.zip`);
  fs.appendFileSync(badZip, "CORRUPT");
  const badSrv = await startServer(badSite, PORT + 1);
  const badMirror = `http://127.0.0.1:${PORT + 1}`;

  // TEST_OLD 薄壳 tgz（改写副本版本字段，不碰仓库里的 npm/package.json）
  const pkgCopy = path.join(WORK, "pkg");
  fs.cpSync(NPM_DIR, pkgCopy, { recursive: true });
  const pkgJson = JSON.parse(fs.readFileSync(path.join(pkgCopy, "package.json"), "utf8"));
  pkgJson.version = TEST_OLD;
  fs.writeFileSync(path.join(pkgCopy, "package.json"), JSON.stringify(pkgJson, null, 2));
  const pack = await npm(["pack", pkgCopy, "--pack-destination", WORK]);
  const tgzName = pack.stdout.trim().split(/\r?\n/).pop();
  const tgz = path.join(WORK, tgzName);
  if (!fs.existsSync(tgz)) fatal(`npm pack 失败：${pack.stdout}${pack.stderr}`);
  const tgzNew = path.join(WORK, tgzName.replace(TEST_OLD, TEST_NEW));
  pkgJson.version = TEST_NEW;
  fs.writeFileSync(path.join(pkgCopy, "package.json"), JSON.stringify(pkgJson, null, 2));
  await npm(["pack", pkgCopy, "--pack-destination", WORK]);
  if (!fs.existsSync(tgzNew)) fatal(`npm pack ${TEST_NEW} 失败`);

  const prefix1 = path.join(WORK, "prefix1");

  console.log("\n① 薄壳安装：postinstall 下载 + SHA-256 校验 + 落位");
  let r = await npm(["install", "-g", "--prefix", prefix1, tgz], { PI_FLASH_MIRROR: mirror });
  check("安装成功", r.status === 0, (r.stderr || ""));
  check(`版本戳 = ${TEST_OLD}`, stampOf(prefix1) === TEST_OLD, `实际 ${stampOf(prefix1)}`);
  check(
    "marker 落位（包装目录内容被搬正）",
    fs.existsSync(path.join(payloadDirOf(prefix1), "marker.txt")),
  );

  console.log("\n② 启动：stdio 透传 + 退出码透传");
  r = await nodeRun(launcherOf(prefix1), ["--version"], { PI_FLASH_MIRROR: mirror });
  check("--version 输出", /^v\d+\./m.test(r.stdout), r.stdout);
  r = await nodeRun(launcherOf(prefix1), ["-e", "console.log('OUT-OK'); process.exit(7)"], {
    PI_FLASH_MIRROR: mirror,
  });
  check("stdout 透传", r.stdout.includes("OUT-OK"), r.stdout);
  check("退出码 7 透传", r.status === 7, `实际 ${r.status}`);

  console.log("\n③ 删载荷自愈 / ④ 版本戳不符自愈");
  fs.rmSync(payloadDirOf(prefix1), { recursive: true, force: true });
  r = await nodeRun(launcherOf(prefix1), ["--version"], { PI_FLASH_MIRROR: mirror });
  check("删除后首启补下", r.status === 0 && stampOf(prefix1) === TEST_OLD, r.stderr);
  // 081 语义：戳比包版本新 = 应用内更新先行，不修；戳更旧才补下 → 用 9.9.0
  fs.writeFileSync(path.join(payloadDirOf(prefix1), ".payload-version"), "9.9.0\n");
  await nodeRun(launcherOf(prefix1), ["--version"], { PI_FLASH_MIRROR: mirror });
  check(`戳修复回 ${TEST_OLD}`, stampOf(prefix1) === TEST_OLD, `实际 ${stampOf(prefix1)}`);

  console.log("\n⑤ --ignore-scripts 安装 → 首启自愈");
  const prefix2 = path.join(WORK, "prefix2");
  r = await npm(
    ["install", "-g", "--prefix", prefix2, "--ignore-scripts", tgz],
    { PI_FLASH_MIRROR: mirror },
  );
  check("安装成功且无载荷", r.status === 0 && stampOf(prefix2) === null);
  r = await nodeRun(launcherOf(prefix2), ["--version"], { PI_FLASH_MIRROR: mirror });
  check("首启自愈补齐", r.status === 0 && stampOf(prefix2) === TEST_OLD, r.stderr);

  console.log("\n⑥ 无 sidecar → SHA256SUMS 兜底");
  const sidecar = path.join(site, `v${TEST_OLD}`, `pi-flash-${TEST_OLD}-win32-x64.zip.sha256`);
  const sidecarBak = `${sidecar}.bak`;
  fs.renameSync(sidecar, sidecarBak);
  const prefix3 = path.join(WORK, "prefix3");
  r = await npm(["install", "-g", "--prefix", prefix3, tgz], { PI_FLASH_MIRROR: mirror });
  check("SHA256SUMS 校验通过", r.status === 0 && stampOf(prefix3) === TEST_OLD, (r.stderr || "").slice(-300));
  fs.renameSync(sidecarBak, sidecar);

  console.log("\n⑦ 损坏载荷必须被拦截（安装失败、无半截残留）");
  const prefix4 = path.join(WORK, "prefix4");
  r = await npm(["install", "-g", "--prefix", prefix4, tgz], { PI_FLASH_MIRROR: badMirror });
  check("安装失败", r.status !== 0);
  check("报 SHA-256 不符", /SHA-256/.test(`${r.stdout}${r.stderr}`));
  check("无半截载荷残留", stampOf(prefix4) === null);

  console.log(`\n⑧ 更新 ${TEST_OLD} → ${TEST_NEW}（同一条命令）`);
  r = await npm(["install", "-g", "--prefix", prefix1, tgzNew], { PI_FLASH_MIRROR: mirror });
  check("安装成功", r.status === 0, (r.stderr || ""));
  check(`版本戳 = ${TEST_NEW}`, stampOf(prefix1) === TEST_NEW, `实际 ${stampOf(prefix1)}`);
  const marker = fs.readFileSync(path.join(payloadDirOf(prefix1), "marker.txt"), "utf8");
  check("载荷内容已换新", marker.includes(`payload-v${TEST_NEW}`), marker);

  console.log("\n⑨ 同版本快路径：镜像不可达也不该触发下载");
  const t0 = Date.now();
  r = await nodeRun(launcherOf(prefix1), ["--version"], { PI_FLASH_MIRROR: "http://127.0.0.1:1" });
  const ms = Date.now() - t0;
  check(`直接启动（${ms}ms）`, r.status === 0 && ms < 5000, r.stderr);

  console.log("\n⑩ 081 应用内更新落位：staging + 标记 → 启动时 swap");
  const prefix5 = path.join(WORK, "prefix5");
  r = await npm(["install", "-g", "--prefix", prefix5, tgz], { PI_FLASH_MIRROR: mirror });
  check(`安装 ${TEST_OLD} 成功`, r.status === 0 && stampOf(prefix5) === TEST_OLD, (r.stderr || ""));
  const pkg5 = path.join(prefix5, "node_modules", "pi-flash");
  const stageDir = path.join(pkg5, `payload.stage-${TEST_NEW}`);
  fs.mkdirSync(stageDir, { recursive: true });
  fs.copyFileSync(process.execPath, path.join(stageDir, "pi-flash.exe"));
  fs.writeFileSync(path.join(stageDir, "marker.txt"), `payload-v${TEST_NEW}\n`);
  fs.writeFileSync(path.join(stageDir, ".payload-version"), `${TEST_NEW}\n`);
  fs.writeFileSync(
    path.join(pkg5, "update-staged.json"),
    JSON.stringify({ version: TEST_NEW, staged_at: 1 }),
  );
  r = await nodeRun(launcherOf(prefix5), ["--version"], { PI_FLASH_MIRROR: mirror });
  check("swap 后启动成功", r.status === 0, r.stderr);
  check(`版本戳 = ${TEST_NEW}`, stampOf(prefix5) === TEST_NEW, `实际 ${stampOf(prefix5)}`);
  check(
    "载荷内容已换新",
    fs.readFileSync(path.join(payloadDirOf(prefix5), "marker.txt"), "utf8").includes(`payload-v${TEST_NEW}`),
  );
  check("标记已清除", !fs.existsSync(path.join(pkg5, "update-staged.json")));
  check(
    "staging/old 目录无残留",
    fs
      .readdirSync(pkg5)
      .filter((e) => e.startsWith("payload.stage-") || e.startsWith("payload.old-"))
      .length === 0,
  );
  check("更新日志行打印", /已应用更新/.test(r.stderr), r.stderr);

  console.log("\n⑪ 载荷比薄壳新：不降级、不重复下载");
  const t1 = Date.now();
  r = await nodeRun(launcherOf(prefix5), ["--version"], { PI_FLASH_MIRROR: "http://127.0.0.1:1" });
  const ms1 = Date.now() - t1;
  check(`直接启动（${ms1}ms）`, r.status === 0 && ms1 < 5000, r.stderr);
  check(`载荷未被降级回 ${TEST_OLD}`, stampOf(prefix5) === TEST_NEW, `实际 ${stampOf(prefix5)}`);
  check(
    `载荷内容保持 ${TEST_NEW}`,
    fs.readFileSync(path.join(payloadDirOf(prefix5), "marker.txt"), "utf8").includes(`payload-v${TEST_NEW}`),
  );

  siteSrv.close();
  badSrv.close();
  if (!KEEP) fs.rmSync(WORK, { recursive: true, force: true });

  console.log(
    failures === 0
      ? "\n全部通过 ✓"
      : `\n✗ ${failures} 项失败${KEEP ? `（工作目录保留：${WORK}）` : ""}`,
  );
  process.exit(failures === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(`✗ 异常：${error.stack || error}`);
  process.exit(1);
});
