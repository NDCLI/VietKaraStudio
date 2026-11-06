"""Generate the 32-color role icons used by the Python ASS renderer."""

from pathlib import Path

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "crates" / "vietkara-render" / "assets" / "role-icons"
OUTPUT = ROOT / "crates" / "vietkara-render" / "assets" / "role-icons-32"
OUTPUT.mkdir(parents=True, exist_ok=True)

for source in sorted(SOURCE.glob("*.png")):
    with Image.open(source) as image:
        quantized = image.convert("RGBA").quantize(
            colors=32, method=Image.Quantize.FASTOCTREE
        ).convert("RGBA")
        quantized.save(OUTPUT / source.name)

print(f"Generated {len(list(OUTPUT.glob('*.png')))} baseline-compatible role icons in {OUTPUT}")
