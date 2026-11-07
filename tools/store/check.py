#!/usr/bin/env python3
"""Check the store copy in docs/store/listing.md against each store's limits.

Every `code span` followed by a bracketed count, and every fenced block under
a "(limit N)" heading, is measured; a field over its limit, or a bracketed
count that is wrong, fails. CI runs it.
"""

import re
import sys
from pathlib import Path

LISTING = Path(__file__).resolve().parents[2] / "docs" / "store" / "listing.md"


def main():
    text = LISTING.read_text(encoding="utf-8")
    bad = 0
    limit = None
    lines = text.splitlines()
    i = 0
    while i < len(lines):
        line = lines[i]
        if "(no limit" in line:
            limit = None
        # A field's limit is the last "(limit N)" before it, on its line or above.
        marks = [(m.start(), int(m.group(1))) for m in re.finditer(r"\(limit (\d+)", line)]
        for m in re.finditer(r"`([^`]+)` \[(\d+)\]", line):
            limit = next((n for at, n in reversed(marks) if at < m.start()), limit)
            value, count = m.group(1), m.group(2)
            n = len(value)
            ok = n == int(count) and (limit is None or n <= limit)
            print(f"{'ok ' if ok else 'BAD'} {n:>4}/{limit} {value[:60]}")
            bad += not ok
        if marks:
            limit = marks[-1][1]
        if line.startswith("```") and limit:
            block = []
            i += 1
            while not lines[i].startswith("```"):
                block.append(lines[i])
                i += 1
            n = len("\n".join(block))
            ok = n <= limit
            print(f"{'ok ' if ok else 'BAD'} {n:>4}/{limit} {block[0][:60]}")
            bad += not ok
            limit = None
        i += 1
    if bad:
        sys.exit(f"{bad} store field(s) over their limit or miscounted")


if __name__ == "__main__":
    main()
