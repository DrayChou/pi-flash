# pi-flash 品牌标志

字母 **P**（左）+ 闪电（右），整体右倾 15°，闪电为视觉主角（上下探出 P 边界）。

## 文件

| 文件 | 用途 |
|---|---|
| `symbol.svg` | 主标志（335×256，横排版）——文档、README、横幅 |
| `tile-256.svg` | 紧凑瓦片（256×256，黑底白标）——一般图标场合 |
| `tile-1024.svg` | .ico / .icns 渲染母版（Big Sur 网格：1024 画布、824 圆角方形、r185） |
| `../../crates/app/assets/icons/logo-marks.svg` | app 内用 marks（无底，256 方画布，gpui 按 alpha 着色） |
| `../../crates/app/assets/icon/pi-flash.ico` | Windows exe 资源（build.rs 经 winresource 嵌入，16–256 九档） |
| `gen_ico.py` | 从 pi-flash-512.png 重生成 .ico 的脚本（九档满画布） |
| `../../crates/app/assets/macos/pi-flash.icns` | macOS .app（release-macos.yml 拷入 Resources） |

## 几何定案（v3，2026-10）

- 倾角 `skewX(-15°)`（竖笔边缘恰为 105° 标准角）；闪电竖向拉伸 sx=7 / sy=9.5，高 184u 对 P 的 160u，上下各探出 ~12u（P 高的 7.5%）
- P↔闪电最小间隙 26u @ y≈125（剪切后空间）＝ 竖笔宽 36u 的 72%，字距经验带（粗展示体 55–75% + 曲线对尖角取宽档）
- 宽度比 闪电/P = 0.72（bbox），墨量比 0.74，高度比 1.15；闪电笔画横厚 49u = 竖笔 1.36×（斜笔光学补偿带 1.3–1.45×）
- 已知取舍：闪电两条长边与 120° 差 1.8–2.1°，系 15° 剪切作用于 45° 之字形的固有结果，不可见，不修

## Windows .ico 满画布约定（2026-10）

初版 .ico 大档沿用 Big Sur 网格留白（图案只占画布 ~82%），Windows 任务栏从大档
降采样绘制，留白一起缩进去，徽章比别家小一圈（实测 Zed 36px、PF 仅 30px）。
Windows 惯例是图案顶满画格：.ico 九档一律由 `gen_ico.py` 从 512 源图裁边重生成
（填充率 100%）；**Big Sur 留白只属于 macOS .icns，不要带进 .ico**。
