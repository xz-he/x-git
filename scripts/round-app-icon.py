"""Apply the existing rounded silhouette to the icon's alpha channel.

Run this script, then `npm run tauri icon -- public/app-icon.png`,
then this script with --finalize-windows.
Requires Pillow. The Git artwork and colors remain unchanged.
"""

from pathlib import Path
import sys

from PIL import Image, ImageChops, ImageDraw


def finalize_windows_icon():
    path = Path(__file__).resolve().parents[1] / "src-tauri" / "icons" / "icon.ico"
    with Image.open(path) as icon:
        sizes = sorted(icon.ico.sizes())
        frames = [icon.ico.getimage(size).convert("RGBA") for size in sizes]
    for frame in frames:
        # Remove faint alpha ringing introduced when resampling the outer corners.
        frame.putalpha(frame.getchannel("A").point(lambda alpha: 0 if alpha <= 4 else alpha))
    frames[-1].save(path, format="ICO", sizes=sizes, append_images=frames[:-1])
    print(f"Finalized transparent Windows icon: {path}")


def main():
    if "--finalize-windows" in sys.argv:
        finalize_windows_icon()
        return
    source = Path(__file__).resolve().parents[1] / "public" / "app-icon.png"
    icon = Image.open(source).convert("RGBA")
    width, height = icon.size
    if width != height:
        raise ValueError("Application icon must be square")
    scale = 4
    mask = Image.new("L", (width * scale, height * scale))
    ImageDraw.Draw(mask).rounded_rectangle(
        (0, 0, width * scale - 1, height * scale - 1),
        radius=width * scale / 4,
        fill=255,
    )
    mask = mask.resize(icon.size, Image.Resampling.LANCZOS)
    icon.putalpha(ImageChops.darker(icon.getchannel("A"), mask))
    icon.save(source)
    print(f"Rounded transparent icon: {source} ({width}x{height})")


if __name__ == "__main__":
    main()
