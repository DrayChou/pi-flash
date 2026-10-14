---
name: video-to-gif
description: 演示视频(mp4)转 GIF 并替换老版本动画的固定流程：ffprobe 探参数 → 查老 GIF 引用处定目标名 → ffmpeg 调色板两遍法转换 → 抽帧验收 → 原名替换。凡是要把 mp4/演示视频转成 GIF、更新 README 演示动画、提到 视频转gif/转gif/演示动画更新/换演示图，一律先按本 skill 操作。
---

# 演示视频转 GIF

把 `docs/images/` 里的演示 mp4 转成 GIF，替换 README 等处引用的老版动画。
单命令 palettegen/paletteuse 即可（本项目实测 ffmpeg 7.x N-126239 可用，无需
ImageMagick）。

## 铁律

- **必须走调色板两遍法**（`palettegen` + `paletteuse`），禁用默认 256 色直转
  ——演示录屏大面积纯色，直转必出明显色带。
- **先探后转**：先 `ffprobe` 拿分辨率/帧率/时长，不要猜。
- **转完必须抽帧验收**：抽中间帧用 Read 看一眼（文字是否清晰、有无花屏），
  通过后才允许替换老文件。
- **替换 = 原名覆盖**：目标 GIF 名以引用处为准（README 引用什么名就叫什么），
  不要按视频文件名另起新名——否则引用全断。
- 转换产物是临时文件，**验收通过替换后必须清理**；默认不提交 git，报告改动
  等用户验收。

## 标准流程

### 0. 确认目标名（替换谁）

```bash
ls docs/images/                                   # 现有 mp4 + 老 gif
grep -rn "pf-intro" --include="*.md" . | grep -v vendor   # 老 gif 被谁引用
```

视频名（如 `017演示.mp4`）和 GIF 名（如 `pf-intro.gif`）通常不一致，
**以 README/文档里的引用名为准**，不要自作主张改引用。

### 1. 探测视频参数

```bash
ffprobe -v error -select_streams v:0 \
  -show_entries stream=width,height,r_frame_rate,duration \
  -of default=noprint_wrappers=1 "docs/images/017演示.mp4"
```

### 2. 转换（单命令内分光调色板）

```bash
cd docs/images
ffmpeg -y -v error -i "017演示.mp4" \
  -vf "fps=12,scale=896:-2:flags=lanczos,split[s0][s1];[s0]palettegen=stats_mode=diff[p];[s1][p]paletteuse=dither=bayer:bayer_scale=4" \
  -loop 0 pf-intro_new.gif
```

参数含义（改前先想清楚）：

| 参数 | 值 | 为什么 |
|---|---|---|
| `fps` | 12 | 源 24fps 对 GIF 太重，12fps 观感无损、体积减半 |
| `scale` | 原宽`:-2` | 保留原始分辨率（`-2` 保证偶数），演示动画别缩 |
| `palettegen stats_mode` | `diff` | 按帧间差异优化色板，录屏场景色彩更准 |
| `paletteuse dither` | `bayer:bayer_scale=4` | 有序抖动，渐变干净且不涨体积（`floyd_steinberg` 体积明显更大） |
| `-loop` | `0` | 无限循环 |

### 3. 验收（不验收不许替换）

```bash
ffprobe -v error -select_streams v:0 \
  -show_entries stream=width,height,nb_frames -of default=noprint_wrappers=1 \
  pf-intro_new.gif
# 抽中间帧（n 从 0 计），然后用 Read 工具看图
ffmpeg -y -v error -i pf-intro_new.gif -vf "select=eq(n\,100)" -vframes 1 tmp_check.png
```

看图检查：文字清晰可读、无色带/花屏、画面完整。**帧数 ≈ 时长 × 12** 对得上
才算整段都转进去了。

### 4. 替换 + 清理

```bash
mv -f pf-intro_new.gif pf-intro.gif
rm -f tmp_check.png
```

## 体积调优（验收不过或太大时）

老版基准 1.7MB，新转 2.1MB 属正常范围。超出 5MB 再按序收紧：

1. `fps` 12 → 10（观感几乎无差，体积约 -17%）
2. `scale` 原宽 → 640（`scale=640:-2`）
3. `bayer_scale` 4 → 5（抖动更粗，体积略降）

反向：若抽帧发现文字发虚，先确认 scale 没缩宽；仍虚则 `fps` 提回 15、
`bayer_scale` 降到 3。
