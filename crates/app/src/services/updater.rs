//! 软件自动更新（081-软件自动更新.md）：揭幕后延迟加载阶段后台检查 GitHub
//! release → on=后台下载→SHA-256 校验→解压 staging（薄壳启动器下次启动
//! swap 落位）→inputpanel 右下版本号位提示；off=版本号「x.x.x↑」可点击开
//! release 页。新版本首启自动开 topbar「更新日志」tab；新会话页版本号
//! 淡字点击 = 手动开同一页（show_changelog）。
//!
//! 落位协议（与 npm 薄壳 080 §3 协作）：
//! ```text
//! <pkg>/payload/                    当前载荷（exe 正在运行，不可整删）
//! <pkg>/payload.stage-<ver>/        下载解压好的新载荷（含 .payload-version 戳）
//! <pkg>/update-staged.json          staging 标记 {"version","staged_at"}——原子写
//! ```
//! swap 只由启动器做（彼时旧进程已退出、目录无锁）：rename payload→old、
//! rename staging→payload、删标记、删 old。exe 从不自己动 payload 目录。
//!
//! 网络纪律（设计文档定）：检测 timeout 30s，失败重试 2 次、间隔 10 分钟，
//! 全程后台线程，绝不阻塞主进程；失败静默回落 Idle，下次启动再来。
//! 下载源与 npm 薄壳同规则：直连优先，开传 3s 平均 <128KB/s 即按序切默认镜像
//! （PI_FLASH_MIRROR 钉定唯一源 / PI_FLASH_MIRRORS 追加候选）。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

/// 下载/检查仓库坐标（与 npm/bin/lib.js DIRECT_BASE 同源）。
const RELEASES_DOWNLOAD_BASE: &str = "https://github.com/danielbill/pi-flash/releases/download";
/// 默认镜像候选（<base>/v<version>/<asset> 同构布局的前缀加速，2026-10 实测可用，
/// 与 npm/bin/lib.js DEFAULT_MIRRORS 同表；失效后 PR 更新两处）。
const DEFAULT_MIRRORS: [&str; 3] = [
    "https://gh-proxy.com/https://github.com/danielbill/pi-flash/releases/download",
    "https://ghfast.top/https://github.com/danielbill/pi-flash/releases/download",
    "https://ghproxy.net/https://github.com/danielbill/pi-flash/releases/download",
];
/// 测速切换（与 npm 薄壳 lib.js 同参数）：开传满 probe（`PI_FLASH_PROBE_MS`，
/// 默认 3s）平均速度仍低于阈值（`PI_FLASH_MIN_KBPS`，默认 128KB/s）即弃源。
const MIN_SPEED_BPS: f64 = 128.0 * 1024.0;
const RELEASES_API_BASE: &str = "https://api.github.com/repos/danielbill/pi-flash/releases";
const UA: &str = "pi-flash-updater";
/// 检测超时（设计文档：30s）。
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// 更新日志现拉超时（用户在等页面，15s 即见「暂时无法连接 GitHub」提示）。
const CHANGELOG_TIMEOUT: Duration = Duration::from_secs(15);
/// 检测失败重试间隔（设计文档：10 分钟 ×2 次）。
const RETRY_INTERVAL: Duration = Duration::from_secs(600);
/// 下载整体预算（zip ≈120MB；连接超时 30s 不封总量）。
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

// ---------------------------------------------------------------------------
// 状态
// ---------------------------------------------------------------------------

/// 版本号位（新会话页 action_bar）的状态机。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum UpdateState {
    /// 无动作：检查中 / 无新版 / 检查失败 / 已是最新。
    Idle,
    /// 后台下载中：转圈 + 「下载 x.x.x 版本中…」（faint）。
    Downloading { version: String },
    /// 新载荷已就位：待重启（「请重启软件切换至 x.x.x 版本」，青蓝）。
    Ready { version: String },
    /// 自动更新 off：有新版（「x.x.x↑」青蓝可点击，开 release 页）。
    Available { version: String },
}

/// 更新日志 tab 的数据（body = markdown；None = 还没拿到）。
#[derive(Debug, Clone)]
pub(crate) struct ChangelogPage {
    pub(crate) version: String,
    pub(crate) body: Option<String>,
    pub(crate) fetching: bool,
    /// 现拉失败（网络不通/被墙，15s 超时）：页面显示「暂时无法连接
    /// GitHub」而非「暂无更新日志」；再点版本号即重试。
    pub(crate) fetch_failed: bool,
}

// ---------------------------------------------------------------------------
// 版本号比较（x.y.z 逐段数值；缺段/非数按 0）
// ---------------------------------------------------------------------------

fn version_key(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches('v')
        .split('.')
        .map(|p| p.trim().parse().unwrap_or(0))
        .collect()
}

/// a 是否严格大于 b。
pub(crate) fn version_gt(a: &str, b: &str) -> bool {
    let (ka, kb) = (version_key(a), version_key(b));
    let n = ka.len().max(kb.len());
    for i in 0..n {
        let (x, y) = (ka.get(i).copied().unwrap_or(0), kb.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

pub(crate) fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

// ---------------------------------------------------------------------------
// 落位协议：pkg 目录 / staging / 标记
// ---------------------------------------------------------------------------

/// npm 包目录（exe 在 `<pkg>/payload/pi-flash.exe`）。开发机 cargo run 时
/// 指向 target/debug 的祖父目录——更新流程在 debug 构建默认关闭，不会写。
fn pkg_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.parent()?.to_path_buf())
}

fn staging_dir(pkg: &Path, version: &str) -> PathBuf {
    pkg.join(format!("payload.stage-{}", version_key_string(version)))
}

fn marker_path(pkg: &Path) -> PathBuf {
    pkg.join("update-staged.json")
}

fn version_key_string(v: &str) -> String {
    v.trim().trim_start_matches('v').to_string()
}

#[derive(Debug, Clone)]
pub(crate) struct StagedUpdate {
    pub version: String,
    pub staged_at: u64,
}

/// 读 staging 标记（结构合法且 staging 目录里有 exe + 吻合的版本戳才算数；
/// 残缺的当垃圾清掉）。
pub(crate) fn read_staged_in(pkg: &Path) -> Option<StagedUpdate> {
    let raw = std::fs::read_to_string(marker_path(pkg)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let version = v
        .get("version")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())?
        .to_string();
    let staged_at = v.get("staged_at").and_then(|x| x.as_u64()).unwrap_or(0);
    let dir = staging_dir(pkg, &version);
    let exe_ok = dir.join("pi-flash.exe").is_file();
    let stamp_ok = std::fs::read_to_string(dir.join(".payload-version"))
        .map(|s| s.trim() == version)
        .unwrap_or(false);
    if exe_ok && stamp_ok {
        return Some(StagedUpdate { version, staged_at });
    }
    // 半截 staging（解压中断/被清）：标记一并作废
    let _ = std::fs::remove_file(marker_path(pkg));
    let _ = std::fs::remove_dir_all(&dir);
    None
}

/// 原子写标记（tmp + rename）。
fn write_staged_in(pkg: &Path, staged: &StagedUpdate) -> std::io::Result<()> {
    let path = marker_path(pkg);
    let tmp = path.with_extension("json.tmp");
    let value = serde_json::json!({
        "version": staged.version,
        "staged_at": staged.staged_at,
    });
    std::fs::write(&tmp, serde_json::to_string(&value).unwrap_or_default())?;
    std::fs::rename(&tmp, &path)
}

/// 启动时初始状态：已有合法 staging = Ready（不花网络）；否则 Idle。
pub(crate) fn initial_state() -> UpdateState {
    let Some(pkg) = pkg_dir() else {
        return UpdateState::Idle;
    };
    match read_staged_in(&pkg) {
        Some(s) => UpdateState::Ready {
            version: s.version,
        },
        None => UpdateState::Idle,
    }
}

// ---------------------------------------------------------------------------
// release 拉取
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct ReleaseInfo {
    pub version: String,
    pub body: String,
}

fn api_base() -> String {
    std::env::var("PI_FLASH_RELEASES_API")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| RELEASES_API_BASE.to_string())
}

/// 候选下载源：`PI_FLASH_MIRROR` 指定唯一源（跳过测速切换）；否则直连优先 +
/// `PI_FLASH_MIRRORS` 追加 + 默认镜像（与 npm/bin/lib.js downloadBases 同规则）。
fn download_bases() -> Vec<String> {
    if let Ok(m) = std::env::var("PI_FLASH_MIRROR") {
        let m = m.trim().trim_end_matches('/').to_string();
        if !m.is_empty() {
            return vec![m];
        }
    }
    let extra = std::env::var("PI_FLASH_MIRRORS")
        .map(|s| {
            s.split([',', ' ', ';'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.trim_end_matches('/').to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut bases = vec![RELEASES_DOWNLOAD_BASE.to_string()];
    bases.extend(extra);
    bases.extend(DEFAULT_MIRRORS.iter().map(|s| s.to_string()));
    bases
}

fn check_agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(CHECK_TIMEOUT).build()
}

/// latest release（含 body = 更新日志 markdown）。tag 形如 v1.2.3。timeout
/// 由调用方定：更新检查走设计文档的 30s，更新日志现拉走 15s。
fn fetch_release(url: &str, timeout: Duration) -> Result<ReleaseInfo, String> {
    let resp = ureq::AgentBuilder::new()
        .timeout(timeout)
        .build()
        .get(url)
        .set("User-Agent", UA)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("{e}"))?;
    // 注意：ureq 2 的 into_string 返回 io::Result（与 call 的 ureq::Error 不同型）
    let text = resp.into_string().map_err(|e| format!("{e}"))?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{e}"))?;
    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or("release 缺 tag_name")?;
    Ok(ReleaseInfo {
        version: version_key_string(tag),
        body: json
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

fn fetch_latest() -> Result<ReleaseInfo, String> {
    fetch_release(&format!("{}/latest", api_base()), CHECK_TIMEOUT)
}

/// 按 tag 拉更新日志（更新日志页缓存缺失时的现拉通道）。15s 超时：被墙/
/// 断网时尽快落「暂时无法连接 GitHub」提示，不让用户对着转圈干等。
fn fetch_release_body(version: &str) -> Result<String, String> {
    let url = format!("{}/tags/v{}", api_base(), version_key_string(version));
    fetch_release(&url, CHANGELOG_TIMEOUT).map(|r| r.body)
}

// ---------------------------------------------------------------------------
// 下载 → 校验 → 解压 staging
// ---------------------------------------------------------------------------

fn asset_name(version: &str) -> String {
    format!("pi-flash-{}-win32-x64.zip", version_key_string(version))
}

/// sha256sum 一行解析（`<hash>  file` / `<hash> *file`，与 lib.js 同规则）。
fn parse_hash_for(text: &str, filename: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        let Some((hash, name)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let hash = hash.trim().to_ascii_lowercase();
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let name = name.trim().trim_start_matches('*').trim();
        let name = name.strip_prefix("./").unwrap_or(name);
        if name == filename {
            return Some(hash);
        }
    }
    None
}

/// zip 条目名安全化：拒绝绝对路径 / `..` / 盘符；分隔符归一。返回 None = 跳过。
fn safe_zip_path(dest: &Path, name: &str) -> Option<PathBuf> {
    let name = name.replace('\\', "/");
    if name.starts_with('/') || name.contains(':') {
        return None;
    }
    let mut out = dest.to_path_buf();
    for part in name.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let f = std::fs::File::open(zip_path).map_err(|e| format!("{e}"))?;
    let mut archive = zip::ZipArchive::new(f).map_err(|e| format!("{e}"))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("{e}"))?;
        let Some(out) = safe_zip_path(dest, entry.name()) else {
            continue;
        };
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| format!("{e}"))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{e}"))?;
        }
        let mut out_f = std::fs::File::create(&out).map_err(|e| format!("{e}"))?;
        std::io::copy(&mut entry, &mut out_f).map_err(|e| format!("{e}"))?;
    }
    Ok(())
}

/// win32 zip 根带 `pi-flash/` 包装目录（release.sh Compress-Archive 行为）；
/// 找到含 pi-flash.exe 的那一层。
fn locate_content_root(extract_dir: &Path) -> Option<PathBuf> {
    for root in [extract_dir.join("pi-flash"), extract_dir.to_path_buf()] {
        if root.join("pi-flash.exe").is_file() {
            return Some(root);
        }
    }
    None
}

/// 下载进度（0-100），None = 无进度数据（未在下载 / 响应无 content-length）。
/// 下载线程 store、新会话页版本号位渲染 load——spinner 动画每帧触发重绘，
/// 原子读足够，无需实体通知。
static DOWNLOAD_PCT: AtomicU8 = AtomicU8::new(PCT_UNKNOWN);
const PCT_UNKNOWN: u8 = 255;

/// 当前下载进度（0-100），供版本号位 UI 与 pif-ui snapshot 读取。
pub(crate) fn download_percent() -> Option<u8> {
    match DOWNLOAD_PCT.load(Ordering::Relaxed) {
        PCT_UNKNOWN => None,
        pct => Some(pct),
    }
}

/// 写进度：None = 未知（哨兵 255）。已知值钳到 100。
fn set_download_percent(pct: Option<u8>) {
    let v = match pct {
        Some(p) => p.min(100),
        None => PCT_UNKNOWN,
    };
    DOWNLOAD_PCT.store(v, Ordering::Relaxed);
}

/// 测速参数（env 可调，与 npm/bin/lib.js 同名变量同默认值；测试注入更小的
/// probe/阈值以秒级完成）。
fn speed_probe() -> Duration {
    Duration::from_millis(
        std::env::var("PI_FLASH_PROBE_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3000),
    )
}

fn min_speed_bps() -> f64 {
    std::env::var("PI_FLASH_MIN_KBPS")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .map(|kbps| kbps * 1024.0)
        .unwrap_or(MIN_SPEED_BPS)
}

/// 下载到文件，边下边算 SHA-256（单遍）。开传满 probe 平均速度仍低于 min_bps
/// 即 Err（测速不过，由上层换源）。返回 hex 摘要。
fn download_and_hash_cfg(
    url: &str,
    dest: &Path,
    probe: Duration,
    min_bps: f64,
) -> Result<String, String> {
    use sha2::Digest;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CHECK_TIMEOUT)
        .timeout(DOWNLOAD_TIMEOUT)
        .build();
    let resp = agent
        .get(url)
        .set("User-Agent", UA)
        .call()
        .map_err(|e| format!("{e}"))?;
    let total: u64 = resp
        .header("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    set_download_percent((total > 0).then_some(0));
    let mut reader = resp.into_reader();
    let mut out = std::fs::File::create(dest).map_err(|e| format!("{e}"))?;
    let mut h = sha2::Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut got: u64 = 0;
    let mut last_pct: u8 = 0;
    let t0 = std::time::Instant::now();
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("{e}"))?;
        if n == 0 {
            break;
        }
        got += n as u64;
        let elapsed = t0.elapsed();
        if elapsed >= probe {
            let speed = got as f64 / elapsed.as_secs_f64();
            if speed < min_bps {
                return Err(format!(
                    "测速不过（{}KB/s < {}KB/s）",
                    (speed / 1024.0) as u64,
                    (min_bps / 1024.0) as u64
                ));
            }
        }
        if total > 0 {
            let pct = ((got as f64 / total as f64) * 100.0).min(100.0) as u8;
            if pct != last_pct {
                set_download_percent(Some(pct));
                last_pct = pct;
            }
        }
        h.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(|e| format!("{e}"))?;
    }
    Ok(format!("{:x}", h.finalize()))
}

/// 按候选源逐个试下载（每源一次，测速不过/网络错误都换下一个），全灭才报错。
/// 返回（zip 的 SHA-256，命中源 base——校验和钉官方优先时兜底用）。
fn download_mirrored(asset: &str, dest: &Path, ver: &str) -> Result<(String, String), String> {
    download_mirrored_cfg(
        asset,
        dest,
        ver,
        &download_bases(),
        speed_probe(),
        min_speed_bps(),
    )
}

fn download_mirrored_cfg(
    asset: &str,
    dest: &Path,
    ver: &str,
    bases: &[String],
    probe: Duration,
    min_bps: f64,
) -> Result<(String, String), String> {
    let mut errors = Vec::new();
    for base in bases {
        eprintln!("[updater] 下载源：{base}");
        match download_and_hash_cfg(&format!("{base}/v{ver}/{asset}"), dest, probe, min_bps) {
            Ok(h) => return Ok((h, base.clone())),
            Err(e) => {
                eprintln!("[updater] 源不可用（{e}），换下一个");
                errors.push(format!("{base}：{e}"));
            }
        }
    }
    Err(format!(
        "所有下载源均失败：{}；可设 PI_FLASH_MIRROR=<base> 指定源，或到 GitHub release 页手动下载",
        errors.join("；")
    ))
}

fn fetch_text(url: &str) -> Result<String, String> {
    let resp = check_agent()
        .get(url)
        .set("User-Agent", UA)
        .call()
        .map_err(|e| format!("{e}"))?;
    resp.into_string().map_err(|e| format!("{e}"))
}

/// 清残留（080 §3.3）：`update.tmp-<pid>` 超过 1 小时未更新即删——活跃下载
/// 持续写盘 mtime 常新，停更 1 小时 = 进程已死；`payload.old-*` 为 swap 中断
/// 遗留，无条件删。npm 薄壳侧另有按 pid 精确判断的清理，这里只做兜底。
fn cleanup_stale_staging(pkg: &Path) {
    const STALE: Duration = Duration::from_secs(3600);
    let Ok(entries) = std::fs::read_dir(pkg) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let stale_tmp = name.starts_with("update.tmp-")
            && entry
                .metadata()
                .and_then(|m| m.modified())
                .map(|mtime| mtime.elapsed().map(|age| age > STALE).unwrap_or(true))
                .unwrap_or(true);
        if stale_tmp || name.starts_with("payload.old-") {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// 下载 + SHA-256 校验（sidecar → SHA256SUMS 兜底，080 §3.2 同序）+ 解压 +
/// rename 成 staging + 写标记。成功返回版本号。
fn stage_update(rel: &ReleaseInfo) -> Result<String, String> {
    let pkg = pkg_dir().ok_or("无法定位安装目录")?;
    std::fs::create_dir_all(&pkg).map_err(|e| format!("{e}"))?;
    cleanup_stale_staging(&pkg);
    let ver = rel.version.clone();
    let tmp = pkg.join(format!("update.tmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("x")).map_err(|e| format!("{e}"))?;

    let result = (|| -> Result<(), String> {
        let asset = asset_name(&ver);
        let zip_path = tmp.join(&asset);

        // 下载（测速自动换源：直连慢/挂就按序切镜像），边下边算 SHA-256
        let (zip_hash, chosen) = download_mirrored(&asset, &zip_path, &ver)?;

        // 校验和：直连优先钉定官方哈希（zip 走镜像时防串包）→ 命中源兜底；
        // sidecar 404 → SHA256SUMS 兜底
        let expected = {
            let mut found = None;
            let mut bases = vec![RELEASES_DOWNLOAD_BASE.to_string()];
            if chosen != RELEASES_DOWNLOAD_BASE {
                bases.push(chosen);
            }
            'outer: for b in &bases {
                for suffix in [format!("{asset}.sha256"), "SHA256SUMS".to_string()] {
                    match fetch_text(&format!("{b}/v{ver}/{suffix}")) {
                        Ok(text) => {
                            if let Some(h) = parse_hash_for(&text, &asset) {
                                found = Some(h);
                                break 'outer;
                            }
                        }
                        Err(_) => continue, // 404 / 网络抖动 → 试下一个来源
                    }
                }
            }
            found.ok_or_else(|| format!("release 资产里找不到 {asset} 的 SHA-256，放弃安装"))?
        };
        if zip_hash != expected {
            return Err(format!("SHA-256 不符（期望 {expected}，实际 {zip_hash}）"));
        }

        extract_zip(&zip_path, &tmp.join("x"))?;
        let root = locate_content_root(&tmp.join("x"))
            .ok_or("载荷结构异常：找不到 pi-flash.exe")?;

        // rename 成 staging（同卷原子）；旧 staging 先清
        let staging = staging_dir(&pkg, &ver);
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::rename(&root, &staging).map_err(|e| format!("staging 落位失败：{e}"))?;
        std::fs::write(staging.join(".payload-version"), format!("{ver}\n"))
            .map_err(|e| format!("{e}"))?;

        // 更新日志缓存（重启后的更新日志页直接读本地）
        if !rel.body.is_empty() {
            if let Some(p) = pi_link::paths::changelog_cache_file(&ver) {
                if let Some(parent) = p.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&p, &rel.body);
            }
        }

        write_staged_in(
            &pkg,
            &StagedUpdate {
                version: ver.clone(),
                staged_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            },
        )
        .map_err(|e| format!("写更新标记失败：{e}"))?;
        Ok(())
    })();

    let _ = std::fs::remove_dir_all(&tmp);
    result.map(|_| ver)
}

// ---------------------------------------------------------------------------
// last_seen（新版本首启判定）
// ---------------------------------------------------------------------------

fn read_last_seen() -> Option<String> {
    let raw = std::fs::read_to_string(pi_link::paths::update_state_file()?).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("last_seen_version")
        .and_then(|x| x.as_str())
        .map(str::to_string)
}

fn write_last_seen(version: &str) {
    let Some(p) = pi_link::paths::update_state_file() else {
        return;
    };
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = pi_link::config::write_json(
        &p,
        &serde_json::json!({ "last_seen_version": version }),
    );
}

fn read_changelog_cache(version: &str) -> Option<String> {
    std::fs::read_to_string(pi_link::paths::changelog_cache_file(version)?)
        .ok()
        .filter(|s| !s.trim().is_empty())
}

// ---------------------------------------------------------------------------
// Chat 接线
// ---------------------------------------------------------------------------

impl crate::Chat {
    /// 调试/开发不跑更新（PI_FLASH_UPDATE_CHECK=1 可强制，0 可关）。
    fn checks_enabled() -> bool {
        match std::env::var("PI_FLASH_UPDATE_CHECK").as_deref() {
            Ok("1") => true,
            Ok("0") => false,
            _ => !cfg!(debug_assertions),
        }
    }

    /// 揭幕后的两个延迟加载钩子（010 §延迟加载「版本更新检查」）：
    /// ① 新版本首启 → 自动开「更新日志」tab；② 后台检查更新。
    /// 关闭时（debug 默认）全程不动，含 last_seen 记账——dev 构建跑一版就
    /// 写 last_seen=新版，正式版更新后首启便永远不算「新版本首启」。
    pub(crate) fn spawn_startup_update_hooks(&mut self, cx: &mut gpui::Context<Self>) {
        if !Self::checks_enabled() {
            return;
        }
        let cur = current_version().to_string();
        match read_last_seen() {
            Some(seen) if version_gt(&cur, &seen) => {
                self.show_changelog(cur.clone(), cx);
                write_last_seen(&cur);
            }
            None => write_last_seen(&cur), // 首次运行：只记账，不开页
            _ => {}
        }
        self.spawn_update_check(cx);
    }

    /// 打开某版本的更新日志页（新版本首启钩子与新会话页版本号点击共用）：
    /// 内存已有同版本页 = 正文在就直接开；拉取中不打断；两者皆无（上次
    /// 拉取失败）置回 fetching 重试。否则读本地缓存装页，缺失则后台按
    /// tag 现拉（只展示不回写缓存——看一次的页面）。
    pub(crate) fn show_changelog(&mut self, version: String, cx: &mut gpui::Context<Self>) {
        let mut need_fetch = false;
        if self
            .aux
            .changelog
            .as_ref()
            .is_some_and(|p| p.version == version)
        {
            if let Some(p) = self.aux.changelog.as_mut() {
                if p.body.is_none() && !p.fetching {
                    p.fetching = true;
                    need_fetch = true;
                }
            }
        } else {
            let cached = read_changelog_cache(&version);
            need_fetch = cached.is_none();
            self.aux.changelog = Some(ChangelogPage {
                version: version.clone(),
                fetching: need_fetch,
                body: cached,
                fetch_failed: false,
            });
        }
        self.open_changelog_tab(cx);
        if need_fetch {
            self.spawn_changelog_fetch(version, cx);
        }
    }

    fn spawn_changelog_fetch(&mut self, version: String, cx: &mut gpui::Context<Self>) {
        cx.spawn(async move |this, cx| {
            let body = cx
                .background_executor()
                .spawn(async move { fetch_release_body(&version) })
                .await;
            let _ = this.update(cx, |c, cx| {
                if let Some(p) = &mut c.aux.changelog {
                    match body {
                        // 空正文 = release 无 notes，走「暂无更新日志」，不算连接失败
                        Ok(b) => {
                            if !b.trim().is_empty() {
                                p.body = Some(b);
                            }
                            p.fetch_failed = false;
                        }
                        Err(_) => p.fetch_failed = true,
                    }
                    p.fetching = false;
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 打开（或聚焦）topbar「更新日志」tab。
    pub(crate) fn open_changelog_tab(&mut self, cx: &mut gpui::Context<Self>) {
        match self
            .ws.panel_tabs
            .iter()
            .position(|t| matches!(t, crate::PanelTab::Changelog))
        {
            Some(ix) => self.activate_panel_tab(ix, cx),
            None => {
                self.ws.panel_tabs.push(crate::PanelTab::Changelog);
                let ix = self.ws.panel_tabs.len() - 1;
                self.activate_panel_tab(ix, cx);
            }
        }
        self.set_content_view(crate::ContentView::Changelog);
        cx.notify();
    }

    /// 后台检查更新（on=下载 staging，off=只提示 ↑）。检测失败重试 2 次、
    /// 间隔 10 分钟（设计文档定），全程不阻塞主进程，最终失败静默。
    pub(crate) fn spawn_update_check(&mut self, cx: &mut gpui::Context<Self>) {
        if !Self::checks_enabled() {
            return;
        }
        // 已有 staging：不花网络，等重启
        if matches!(self.aux.update, UpdateState::Ready { .. }) {
            return;
        }
        cx.spawn(async move |this, cx| {
            // ① 检测（3 次尝试，间隔 10 分钟）
            let mut latest: Option<ReleaseInfo> = None;
            for attempt in 0..3 {
                let r = cx
                    .background_executor()
                    .spawn(async { fetch_latest() })
                    .await;
                match r {
                    Ok(rel) => {
                        latest = Some(rel);
                        break;
                    }
                    Err(e) => {
                        eprintln!("[updater] 检查更新失败（{e}）");
                        if attempt < 2 {
                            cx.background_executor().timer(RETRY_INTERVAL).await;
                        }
                    }
                }
            }
            let Some(rel) = latest else { return };
            if !version_gt(&rel.version, current_version()) {
                return; // 已是最新
            }
            // ② 自动更新开关（读最新设置：用户可能中途改）
            let auto = this
                .update(cx, |_, _| crate::services::workspace::auto_update())
                .unwrap_or(true);
            if !auto {
                let _ = this.update(cx, |c, cx| {
                    c.aux.update = UpdateState::Available {
                        version: rel.version.clone(),
                    };
                    cx.notify();
                });
                return;
            }
            // ③ 后台下载 → staging
            let _ = this.update(cx, |c, cx| {
                c.aux.update = UpdateState::Downloading {
                    version: rel.version.clone(),
                };
                cx.notify();
            });
            let outcome = cx
                .background_executor()
                .spawn(async move { stage_update(&rel) })
                .await;
            let _ = this.update(cx, |c, cx| {
                c.aux.update = match &outcome {
                    Ok(v) => UpdateState::Ready {
                        version: v.clone(),
                    },
                    Err(e) => {
                        eprintln!("[updater] 下载更新失败：{e}");
                        UpdateState::Idle
                    }
                };
                cx.notify();
            });
        })
        .detach();
    }
}

/// 打开该版本的 GitHub release 页（off 模式点击「x.x.x↑」）。
pub(crate) fn open_release_page(version: &str) {
    let url = format!(
        "https://github.com/danielbill/pi-flash/releases/tag/v{}",
        version_key_string(version)
    );
    open_url(&url);
}

fn open_url(url: &str) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = url;
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(version_gt("1.2.3", "1.2.2"));
        assert!(version_gt("v1.2.3", "1.2.2"));
        assert!(version_gt("1.10.0", "1.9.9"), "逐段数值比较，不是字典序");
        assert!(version_gt("0.2.0", "0.1.99"));
        assert!(!version_gt("1.2.3", "1.2.3"));
        assert!(!version_gt("1.2", "1.2.0"), "缺段按 0");
        assert!(!version_gt("0.1.0", "0.2.0"));
        assert!(!version_gt("abc", "0.0.1"), "非数段按 0");
    }

    #[test]
    fn hash_line_parse() {
        let text = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  pi-flash-0.1.2-win32-x64.zip\n\
                    011f68a3a5e3c1fda41e7f68a9a4f1654eeab5d5f4d19c8e13a7d411d1a25b35 *other.zip\n";
        assert_eq!(
            parse_hash_for(text, "pi-flash-0.1.2-win32-x64.zip").unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert!(parse_hash_for(text, "missing.zip").is_none());
        assert!(parse_hash_for("garbage line\n", "x.zip").is_none());
    }

    #[test]
    fn zip_path_sanitize() {
        let dest = Path::new("D:\\pkg");
        assert!(safe_zip_path(dest, "pi-flash/vendor/pi/x.js").is_some());
        assert!(safe_zip_path(dest, "pi-flash\\vendor\\x.js").is_some());
        assert!(safe_zip_path(dest, "/abs/path").is_none(), "绝对路径拒绝");
        assert!(safe_zip_path(dest, "a/../../etc").is_none(), "上跳拒绝");
        assert!(safe_zip_path(dest, "C:\\evil").is_none(), "盘符拒绝");
        assert!(safe_zip_path(dest, "a/./b").is_some(), "点段折叠");
    }

    #[test]
    fn staged_marker_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "pf-updater-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let pkg = dir.join("pkg");
        std::fs::create_dir_all(staging_dir(&pkg, "0.1.2")).unwrap();
        std::fs::write(staging_dir(&pkg, "0.1.2").join("pi-flash.exe"), b"x").unwrap();
        std::fs::write(staging_dir(&pkg, "0.1.2").join(".payload-version"), "0.1.2\n").unwrap();

        // 半截 staging：无标记 → None
        assert!(read_staged_in(&pkg).is_none());

        write_staged_in(
            &pkg,
            &StagedUpdate {
                version: "0.1.2".into(),
                staged_at: 42,
            },
        )
        .unwrap();
        let staged = read_staged_in(&pkg).unwrap();
        assert_eq!(staged.version, "0.1.2");

        // staging 目录被清 → 标记作废并清理
        std::fs::remove_dir_all(staging_dir(&pkg, "0.1.2")).unwrap();
        assert!(read_staged_in(&pkg).is_none());
        assert!(!marker_path(&pkg).exists(), "残缺标记一并清除");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- 下载引擎功能测试（本地裸 TCP 假源，真下载真哈希真守卫） ------------

    /// 起一次性 HTTP 服务返回 base；trickle_ms = 每 1KB 间隔（模拟慢源）；
    /// with_length = false 时无 content-length（close 分隔）。
    fn serve(body: Vec<u8>, trickle_ms: Option<u64>, with_length: bool) -> String {
        use std::io::Write as _;
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let Ok((mut s, _)) = listener.accept() else {
                return;
            };
            let mut req = [0u8; 4096];
            let _ = std::io::Read::read(&mut s, &mut req);
            let len_head = if with_length {
                format!("Content-Length: {}\r\n", body.len())
            } else {
                String::new()
            };
            let _ = write!(s, "HTTP/1.1 200 OK\r\n{len_head}Connection: close\r\n\r\n");
            for chunk in body.chunks(1024) {
                if s.write_all(chunk).is_err() {
                    return; // 测试端测速中止断开属预期
                }
                if let Some(ms) = trickle_ms {
                    std::thread::sleep(Duration::from_millis(ms));
                }
            }
        });
        format!("http://{addr}")
    }

    /// 已关闭的端口（连接拒绝），当挂掉的源。
    fn dead_base() -> String {
        use std::net::TcpListener;
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        drop(l);
        format!("http://{addr}")
    }

    fn test_dest(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("pf-upd-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    /// 顺序跑三种场景（共用全局百分比原子量，避免并行互扰）：
    /// ① 测速守卫弃慢源；② 死源在前自动切换，哈希正确、百分比到 100；
    /// ③ 无 content-length 成功但无百分比。
    #[test]
    fn download_speed_guard_and_failover() {
        use sha2::Digest as _;
        let probe = Duration::from_millis(300);
        let min_bps = 128.0 * 1024.0;

        // ① 20KB/s 慢源 → 快速中止，错误信息含"测速不过"
        let slow = serve(vec![0xAB; 64 * 1024], Some(50), true);
        let t0 = std::time::Instant::now();
        let r = download_and_hash_cfg(
            &format!("{slow}/v0.0.1/x.zip"),
            &test_dest("slow"),
            probe,
            min_bps,
        );
        assert!(r.is_err(), "慢源应被测速守卫拒绝");
        assert!(r.unwrap_err().contains("测速不过"));
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "应秒级中止而非拖到整体超时"
        );

        // ② 死端口在前 → 切到可用源；命中源正确、SHA-256 正确、百分比 100
        let body = vec![0x5Au8; 256 * 1024];
        let good = serve(body.clone(), None, true);
        let bases = vec![dead_base(), good.clone()];
        let (hash, chosen) = download_mirrored_cfg(
            "x.zip",
            &test_dest("fo"),
            "0.0.1",
            &bases,
            probe,
            min_bps,
        )
        .unwrap();
        assert_eq!(chosen, good, "应命中可用源");
        let mut h = sha2::Sha256::new();
        h.update(&body);
        assert_eq!(hash, format!("{:x}", h.finalize()), "边下边算哈希正确");
        assert_eq!(download_percent(), Some(100));

        // ③ 无 content-length：下载成功但百分比保持未知
        let stream = serve(b"no-length-body".to_vec(), None, false);
        let (_h3, chosen3) = download_mirrored_cfg(
            "x.zip",
            &test_dest("nl"),
            "0.0.1",
            &[stream.clone()],
            probe,
            min_bps,
        )
        .unwrap();
        assert_eq!(chosen3, stream);
        assert_eq!(download_percent(), None, "无 content-length 无百分比");
    }

    /// 真实网络端到端（发版前手动跑一次）：
    /// `cargo test -p app updater_real_download -- --ignored --nocapture`
    #[test]
    #[ignore = "真实网络下载整个载荷（约 85MB），手动验证用"]
    fn updater_real_download() {
        let rel = fetch_latest().unwrap_or_else(|e| panic!("fetch_latest 失败：{e}"));
        let asset = asset_name(&rel.version);
        let dir = std::env::temp_dir().join(format!("pf-upd-real-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join(&asset);
        let (hash, chosen) =
            download_mirrored(&asset, &dest, &rel.version).unwrap_or_else(|e| panic!("下载失败：{e}"));
        eprintln!("v{} 命中源：{chosen}\nsha256={hash}\n文件：{}", rel.version, dest.display());
        assert_eq!(download_percent(), Some(100));
        assert!(dest.metadata().unwrap().len() > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
