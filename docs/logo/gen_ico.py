"""从 pi-flash-512.png 重新生成 pi-flash.ico（九档全满画布）。

背景：初版 ico 大尺寸档沿用 Big Sur 网格留白（图案只占画布 ~82%），
Windows 任务栏从大档降采样绘制，留白一起缩进去，图标比别家小一圈
（实测 Zed 徽章 36px、PF 只有 30px）。Windows 惯例是图案顶满画格，
故本脚本裁掉透明边后重采样所有档位；macOS .icns 保留 Big Sur 留白不动。

用法：python docs/logo/gen_ico.py  （依赖 Pillow）
"""

import io
import struct
from pathlib import Path

from PIL import Image

HERE = Path(__file__).parent
SRC = HERE.parent.parent / "crates/app/assets/icon/pi-flash-512.png"
DST = HERE.parent.parent / "crates/app/assets/icon/pi-flash.ico"
SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]


def build_frames() -> list[Image.Image]:
    src = Image.open(SRC).convert("RGBA")
    art = src.crop(src.getchannel("A").getbbox())  # 裁掉 Big Sur 透明留白
    return [art.resize((s, s), Image.LANCZOS) for s in SIZES]


def fill_ratio(img: Image.Image) -> int:
    bb = img.getchannel("A").getbbox()
    return round((bb[2] - bb[0]) * 100 / img.width)


def main() -> None:
    frames = build_frames()
    # Pillow 只收录不大于基底图的档位，基底必须传最大的 256
    base, extras = frames[-1], frames[:-1]
    buf = io.BytesIO()
    base.save(buf, format="ICO", append_images=extras, sizes=[(s, s) for s in SIZES])
    DST.write_bytes(buf.getvalue())

    # 自检：帧数、尺寸、填充率
    data = DST.read_bytes()
    count = struct.unpack("<H", data[4:6])[0]
    assert count == len(SIZES), f"帧数 {count} != {len(SIZES)}"
    for i, s in enumerate(SIZES):
        off = 6 + i * 16
        w, h = struct.unpack("<BB", data[off : off + 2])
        w = w or 256
        assert w == s, f"第 {i} 帧尺寸 {w} != {s}"
        print(f"  {w}x{w}  填充率 {fill_ratio(frames[i])}%")
    print(f"OK -> {DST} ({len(data)} bytes)")


if __name__ == "__main__":
    main()
