"""Parse authored NMS XML assets and reject DTD/entity declarations."""

from __future__ import annotations

import sys
from pathlib import Path
from xml.etree import ElementTree

XML_SUFFIXES = {".exml", ".mxml", ".xml"}


def validate(path: Path) -> None:
    if path.suffix.lower() not in XML_SUFFIXES:
        return
    data = path.read_bytes()
    upper = data.upper()
    if b"<!DOCTYPE" in upper or b"<!ENTITY" in upper:
        raise ValueError("DTD and entity declarations are not valid mod assets")
    ElementTree.fromstring(data)


def main() -> int:
    failures = []
    for name in sys.argv[1:]:
        path = Path(name)
        try:
            validate(path)
        except (OSError, ElementTree.ParseError, ValueError) as error:
            failures.append(f"{path}: {error}")
    if failures:
        print("SPSP XML asset validation failed:", file=sys.stderr)
        print("\n".join(f"- {failure}" for failure in failures), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
