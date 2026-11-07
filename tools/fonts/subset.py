#!/usr/bin/env python3
"""Cut the Japanese and Simplified Chinese faces down to the characters the
game uses (T65).

The full Noto Sans JP and SC faces are 10 and 18 MB: far too much for a phone
download or the web build. The game only ever shows the strings in
`crates/ui/lang/*.txt`, so each face keeps just the characters its language
uses, plus its language's own name for the settings button, at the weight of
the Latin face (SemiBold, 600).

Run it again whenever ja.txt or zh-Hans.txt gains a character; the test
`every_character_has_a_glyph` in crates/ui says when that is needed.

    pip install fonttools
    python3 tools/fonts/subset.py NotoSansJP[wght].ttf NotoSansSC[wght].ttf

The variable fonts come from https://github.com/google/fonts (ofl/notosansjp,
ofl/notosanssc), under the SIL Open Font License.
"""

import pathlib
import sys

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

ROOT = pathlib.Path(__file__).resolve().parents[2]
LANG = ROOT / "crates" / "ui" / "lang"
OUT = ROOT / "assets" / "fonts"

# Each face: the source font, the strings it must show, and its file.
FACES = [
    ("ja.txt", "日本語", "NotoSansJP-SemiBold-subset.ttf"),
    ("zh-Hans.txt", "简体中文", "NotoSansSC-SemiBold-subset.ttf"),
]


def characters(table: str, extra: str) -> str:
    """Every character outside ASCII in the table's texts, and `extra`."""
    text = extra
    for line in (LANG / table).read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            text += line.split("=", 1)[1]
    return "".join(sorted({c for c in text if ord(c) > 0x7F}))


def cut(source: str, chars: str, out: pathlib.Path) -> None:
    font = TTFont(source)
    if "fvar" in font:
        font = instancer.instantiateVariableFont(font, {"wght": 600})
    options = subset.Options()
    options.layout_features = []
    options.name_IDs = [0, 1, 2, 3, 4, 5, 6, 13, 14]
    options.notdef_outline = True
    options.hinting = False
    sub = subset.Subsetter(options)
    sub.populate(text=chars)
    sub.subset(font)
    font.save(out)
    print(f"{out.name}: {len(chars)} characters, {out.stat().st_size // 1024} KB")


def main() -> None:
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    for source, (table, extra, name) in zip(sys.argv[1:], FACES):
        cut(source, characters(table, extra), OUT / name)


if __name__ == "__main__":
    main()
