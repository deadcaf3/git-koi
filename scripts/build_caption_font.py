"""Rasterize the pond caption's glyphs into assets/caption-font.bin.

The pond draws its caption without a font parser: this script renders Inter Regular and
Inter Medium once, at the caption's pixel size, into alpha masks that the Rust renderer
blits. Run it again only to change the font, size, or character set:

    python3 scripts/build_caption_font.py /path/to/Inter-Regular.otf /path/to/Inter-Medium.otf

The seal's date inscription uses a second, smaller atlas, built the same way:

    python3 scripts/build_caption_font.py REGULAR.otf MEDIUM.otf 22 inscription-font.bin

Inter is licensed under the SIL Open Font License 1.1; see assets/OFL.txt.

File layout, little-endian:
    b"KOIF", u16 version (1), u16 pixel size, u16 ascent, u16 descent, u16 weight count
    then per weight: u16 glyph count, then per glyph:
        u32 code point, i16 left bearing, i16 top above baseline, u16 width, u16 height,
        u16 advance in 1/64 pixel, then width * height alpha bytes, row by row
"""

import struct
import sys
from pathlib import Path

from PIL import ImageFont

PIXEL_SIZE = 30  # 12 logical pixels at the pond's 2.5x export scale
OUTPUT_NAME = "caption-font.bin"
CHARACTERS = [chr(code) for code in range(0x20, 0x7F)] + ["·", "’"]


def encode_weight(font_path: str) -> bytes:
    font = ImageFont.truetype(font_path, PIXEL_SIZE, layout_engine=ImageFont.Layout.BASIC)
    encoded = bytearray(struct.pack("<H", len(CHARACTERS)))
    for character in CHARACTERS:
        advance = round(font.getlength(character) * 64)
        if character == " ":
            encoded += struct.pack("<IhhHHH", ord(character), 0, 0, 0, 0, advance)
            continue
        mask, (left, top) = font.getmask2(character, mode="L", anchor="ls")
        width, height = mask.size
        encoded += struct.pack("<IhhHHH", ord(character), left, -top, width, height, advance)
        encoded += bytes(mask)
    return bytes(encoded)


def main() -> None:
    global PIXEL_SIZE
    if len(sys.argv) not in (3, 5):
        sys.exit(__doc__)
    regular_path, medium_path = sys.argv[1:3]
    output_name = OUTPUT_NAME
    if len(sys.argv) == 5:
        PIXEL_SIZE, output_name = int(sys.argv[3]), sys.argv[4]
    ascent, descent = ImageFont.truetype(regular_path, PIXEL_SIZE).getmetrics()
    output = bytearray(b"KOIF")
    output += struct.pack("<HHHHH", 1, PIXEL_SIZE, ascent, descent, 2)
    output += encode_weight(regular_path)
    output += encode_weight(medium_path)
    output_path = Path(__file__).resolve().parent.parent / "assets" / output_name
    output_path.parent.mkdir(exist_ok=True)
    output_path.write_bytes(output)
    print(f"wrote {output_path} ({len(output)} bytes)")


if __name__ == "__main__":
    main()
