#!/usr/bin/env python3
"""Docs-freshness lint (#85) — catches the drift classes that shipped twice
on 2026-07-04: a stale table of contents, a wrong tool count, and a release
without a migration note. No LLM, runs in milliseconds, fails loud.

Checks:
  1. Every README.md internal anchor resolves to a real heading and the root
     landing page stays within its concise presentation budget.
  2. "N tools" claims in GLOSSARY.md/README.md match the number of #[tool(
     definitions under helixir/src/mcp/tools/.
  3. Documented schema/MCP counts match their authoritative sources.
  4. UPGRADING.md mentions the current minor version from Cargo.toml.
  5. Known removed schema names do not reappear as current contracts.
  6. Maintained Markdown files contain no broken local links and Mermaid
     blocks have a supported diagram declaration.
"""

import glob
import re
import sys
from pathlib import Path
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parent.parent.parent
failures = []
README_LINE_BUDGET = 600

MAINTAINED_DOCS = [
    ROOT / "README.md",
    ROOT / "GLOSSARY.md",
    ROOT / "UPGRADING.md",
    ROOT / "AGENTS.md",
    ROOT / "integration/README.md",
    ROOT / "integration/AGENTS.md",
    ROOT / "integration/SKILLS.md",
    ROOT / "helixir/skills/helixir-memory/SKILL.md",
    ROOT / "helixir/src/mcp/prompts/cognitive_protocol.md",
    *sorted((ROOT / "helixir/doc").glob("*.md")),
    *sorted((ROOT / "helixir/doc/v0.17.0").glob("*.md")),
]


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


def check_internal_anchors():
    readme = (ROOT / "README.md").read_text()
    if len(readme.splitlines()) > README_LINE_BUDGET:
        fail(
            "README: product landing page exceeds the "
            f"{README_LINE_BUDGET}-line presentation budget"
        )
    headings = re.findall(r"^(#{2,4})\s+(.+)$", readme, re.M)
    anchors = [github_anchor(h) for _, h in headings]
    heading_set = set(anchors)

    for label, anchor in re.findall(r"\[([^\]]+)\]\(#([^)]+)\)", readme):
        if anchor not in heading_set:
            fail(f"README: anchor '#{anchor}' ('{label}') has no matching heading")
    for anchor in re.findall(r'href="#([^"]+)"', readme):
        if anchor not in heading_set:
            fail(f"README: HTML anchor '#{anchor}' has no matching heading")


def markdown_anchors(path):
    text = path.read_text()
    return {
        github_anchor(heading)
        for _, heading in re.findall(r"^(#{1,6})\s+(.+)$", text, re.M)
    }


def check_local_links_and_mermaid():
    supported_mermaid = (
        "flowchart",
        "graph",
        "sequenceDiagram",
        "classDiagram",
        "stateDiagram",
        "erDiagram",
        "journey",
        "gantt",
        "pie",
        "mindmap",
        "timeline",
    )
    for path in MAINTAINED_DOCS:
        text = path.read_text()
        for target in re.findall(r"!?\[[^\]]*\]\(([^)]+)\)", text):
            target = target.strip().strip("<>")
            if not target or target.startswith(("http://", "https://", "mailto:", "#")):
                continue
            target = target.split(maxsplit=1)[0]
            relative, _, fragment = target.partition("#")
            resolved = (path.parent / unquote(relative)).resolve()
            if not resolved.exists():
                fail(f"{path.relative_to(ROOT)}: local link target does not exist: {target}")
                continue
            if fragment and resolved.is_file() and resolved.suffix.lower() == ".md":
                if unquote(fragment) not in markdown_anchors(resolved):
                    fail(
                        f"{path.relative_to(ROOT)}: anchor '#{fragment}' does not exist in "
                        f"{resolved.relative_to(ROOT)}"
                    )

        blocks = re.findall(r"```mermaid\s*\n(.*?)\n```", text, re.S)
        if text.count("```mermaid") != len(blocks):
            fail(f"{path.relative_to(ROOT)}: unclosed Mermaid fence")
        for block in blocks:
            source_lines = [
                line.strip()
                for line in block.splitlines()
                if line.strip() and not line.lstrip().startswith("%%")
            ]
            if not source_lines or not source_lines[0].startswith(supported_mermaid):
                fail(f"{path.relative_to(ROOT)}: unsupported or missing Mermaid declaration")


def check_tool_count():
    actual = 0
    for f in glob.glob(str(ROOT / "helixir/src/mcp/tools/*.rs")):
        actual += Path(f).read_text().count("#[tool(")
    for doc in ["GLOSSARY.md", "README.md", "AGENTS.md"]:
        text = (ROOT / doc).read_text()
        for n in re.findall(r"\b(\d+) tools\b", text):
            if int(n) != actual:
                fail(f"{doc}: claims '{n} tools' but the server exposes {actual}")


def check_contract_counts():
    schema = (ROOT / "helixir/schema/schema.hx").read_text()
    tool_count = sum(
        Path(path).read_text().count("#[tool(")
        for path in glob.glob(str(ROOT / "helixir/src/mcp/tools/*.rs"))
    )
    counts = {
        "node": len(re.findall(r"^N::", schema, re.M)),
        "edge": len(re.findall(r"^E::", schema, re.M)),
        "vector": len(re.findall(r"^V::", schema, re.M)),
        "query": len(
            re.findall(r"^QUERY\s+", (ROOT / "helixir/schema/queries.hx").read_text(), re.M)
        ),
        "prompt": len(
            re.findall(r"^\s*#\[prompt\(", (ROOT / "helixir/src/mcp/handler.rs").read_text(), re.M)
        ),
        "resource": len(
            re.findall(r"RawResource::new\(", (ROOT / "helixir/src/mcp/handler.rs").read_text())
        ),
    }
    claims = {
        "README.md": [
            (rf"\b{counts['node']} node types\b", "node count"),
            (rf"\b{counts['edge']} edge types\b", "edge count"),
        ],
        "helixir/doc/architecture.md": [
            (rf"\b{counts['query']} HQL queries\b", "query count"),
            (rf"\b{counts['node']} nodes / {counts['edge']} edges\b", "schema counts"),
        ],
        "helixir/doc/userflow.md": [
            (
                rf"There are {tool_count} tools, {counts['prompt']} prompts, and {counts['resource']} resources\.",
                "MCP prompt/resource counts",
            )
        ],
        "AGENTS.md": [
            (rf"The MCP surface contains {tool_count} tools\.", "agent-guide tool count")
        ],
    }
    for relative, expected in claims.items():
        text = (ROOT / relative).read_text()
        for pattern, label in expected:
            if not re.search(pattern, text):
                fail(f"{relative}: missing current {label} ({counts})")


def check_removed_contracts():
    current_docs = [
        ROOT / "helixir/doc/architecture.md",
        ROOT / "helixir/doc/dataflow.md",
        ROOT / "helixir/doc/design-rationale.md",
    ]
    removed = [
        "BELONGS_TO_CATEGORY",
        "NEXT_CHUNK",
        "APPLIES_IN",
        "IN_SESSION",
        "PAGE_TO_CHUNK",
        "CHUNK_MENTIONS_CONCEPT",
        "CONCEPT_HAS_EXAMPLE",
        "ERROR_REFERENCES_CONCEPT",
        "CATEGORY_HAS_EMBEDDING",
    ]
    for path in current_docs:
        text = path.read_text()
        for name in removed:
            if name in text:
                fail(f"{path.relative_to(ROOT)}: removed schema name {name} is presented in an evergreen doc")

    stale_claims = {
        ROOT / "helixir/doc/dataflow.md": ["Soft-delete via `is_deleted` flag"],
        ROOT / "helixir/doc/design-rationale.md": [
            "commit_partial writes a Memory",
            "scheduled to become SUPERSEDE-only",
        ],
        ROOT / "README.md": [
            "one batched HQL call per depth level",
            "no LLM configured at all",
        ],
    }
    for path, phrases in stale_claims.items():
        text = path.read_text()
        for phrase in phrases:
            if phrase in text:
                fail(f"{path.relative_to(ROOT)}: stale contract phrase: {phrase}")


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


check_internal_anchors()
check_local_links_and_mermaid()
check_tool_count()
check_contract_counts()
check_removed_contracts()
check_upgrading()

if failures:
    print("docs-lint FAILED:")
    for f in failures:
        print(f"  ✗ {f}")
    sys.exit(1)
print("docs-lint: landing budget, links, Mermaid, schema/MCP counts, removed contracts and upgrade freshness are consistent")
