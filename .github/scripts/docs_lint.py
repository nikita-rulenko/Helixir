#!/usr/bin/env python3
"""Docs-freshness lint (#85) — catches the drift classes that shipped twice
on 2026-07-04: a stale table of contents, a wrong tool count, and a release
without a migration note. No LLM, runs in milliseconds, fails loud.

Checks:
  1. README.md Contents block: every internal anchor resolves to a real
     heading (GitHub anchor rules), and listed sub-items appear in the same
     order as their headings do in the body.
  2. "N tools" claims in GLOSSARY.md/README.md match the number of #[tool(
     definitions under helixir/src/mcp/tools/.
  3. UPGRADING.md mentions the current minor version from Cargo.toml.
"""

import glob
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
failures = []


def fail(msg):
    failures.append(msg)


def github_anchor(heading):
    """GitHub's anchor algorithm: lowercase, strip punctuation, spaces->dashes."""
    text = re.sub(r"[*_`]", "", heading.strip())
    text = text.lower()
    text = re.sub(r"[^\w\sЀ-ӿ-]", "", text)
    # GitHub replaces EACH space with a dash (no collapsing): a removed
    # em-dash between words yields a double dash, e.g. "memory--the-moirai".
    return text.strip().replace(" ", "-")


def check_toc():
    readme = (ROOT / "README.md").read_text()
    headings = re.findall(r"^(#{2,4})\s+(.+)$", readme, re.M)
    anchors = [github_anchor(h) for _, h in headings]
    heading_order = {a: i for i, a in enumerate(anchors) if a not in {}}

    m = re.search(r"## Contents\n(.*?)\n---", readme, re.S)
    if not m:
        fail("README: no Contents block found")
        return
    toc_links = re.findall(r"\[([^\]]+)\]\(#([^)]+)\)", m.group(1))
    prev_pos = -1
    for label, anchor in toc_links:
        if anchor not in heading_order:
            fail(f"README TOC: anchor '#{anchor}' ('{label}') has no matching heading")
            continue
        pos = heading_order[anchor]
        if pos < prev_pos:
            fail(
                f"README TOC: '{label}' (#{anchor}) is listed out of order "
                "relative to the previous TOC entry's heading position"
            )
        prev_pos = pos


def check_tool_count():
    actual = 0
    for f in glob.glob(str(ROOT / "helixir/src/mcp/tools/*.rs")):
        actual += Path(f).read_text().count("#[tool(")
    for doc in ["GLOSSARY.md", "README.md"]:
        text = (ROOT / doc).read_text()
        for n in re.findall(r"the (\d+) tools", text):
            if int(n) != actual:
                fail(f"{doc}: claims '{n} tools' but the server exposes {actual}")


def check_upgrading():
    cargo = (ROOT / "helixir/Cargo.toml").read_text()
    m = re.search(r'^version\s*=\s*"(\d+)\.(\d+)\.', cargo, re.M)
    if not m:
        fail("Cargo.toml: cannot parse version")
        return
    minor = f"v{m.group(1)}.{m.group(2)}"
    upgrading = (ROOT / "UPGRADING.md").read_text()
    if minor not in upgrading:
        fail(
            f"UPGRADING.md: no mention of the current version line {minor} — "
            "a release shipped without a migration note"
        )


check_toc()
check_tool_count()
check_upgrading()

if failures:
    print("docs-lint FAILED:")
    for f in failures:
        print(f"  ✗ {f}")
    sys.exit(1)
print("docs-lint: TOC anchors, tool count and UPGRADING freshness all consistent")
