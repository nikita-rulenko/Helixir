#!/usr/bin/env python3
"""Render Helixir Homebrew formulae from immutable release checksums."""

from __future__ import annotations

import argparse
import pathlib
import re
import sys
from urllib.parse import urlparse


SERVER_ASSETS = {
    "macos_arm": "helixir-macos-arm64.tar.gz",
    "macos_intel": "helixir-macos-x86_64.tar.gz",
    "linux_arm": "helixir-linux-arm64.tar.gz",
    "linux_intel": "helixir-linux-x86_64.tar.gz",
}

CLIENT_ASSETS = {
    "macos_arm": "helixir-client-macos-arm64.tar.gz",
    "macos_intel": "helixir-client-macos-x86_64.tar.gz",
    "linux_arm": "helixir-client-linux-arm64.tar.gz",
    "linux_intel": "helixir-client-linux-x86_64.tar.gz",
}


def parse_checksums(path: pathlib.Path, assets: dict[str, str]) -> dict[str, str]:
    checksums: dict[str, str] = {}
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        parts = line.split()
        if len(parts) != 2 or not re.fullmatch(r"[0-9a-fA-F]{64}", parts[0]):
            raise ValueError(f"invalid checksum line {number}: {line!r}")
        checksums[pathlib.Path(parts[1].lstrip("*")).name] = parts[0].lower()
    missing = sorted(set(assets.values()) - checksums.keys())
    if missing:
        raise ValueError(f"missing release checksums: {', '.join(missing)}")
    return checksums


def validate_base_url(base_url: str) -> str:
    if any(character in base_url for character in ['"', "\n", "\r"]):
        raise ValueError("base URL contains an unsafe character")
    parsed = urlparse(base_url)
    if parsed.scheme not in {"https", "file"}:
        raise ValueError("base URL must use https or file")
    if parsed.scheme == "https" and not parsed.netloc:
        raise ValueError("https base URL must contain a host")
    if parsed.scheme == "file" and (parsed.netloc or not parsed.path.startswith("/")):
        raise ValueError("file base URL must be an absolute local URL")
    return base_url.rstrip("/")


def render(
    tag: str,
    repository: str,
    checksums: dict[str, str],
    package: str,
    base_url: str | None = None,
) -> str:
    if not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?", tag):
        raise ValueError(f"invalid release tag: {tag!r}")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError(f"invalid GitHub repository: {repository!r}")
    version = tag.removeprefix("v")
    base = validate_base_url(
        base_url or f"https://github.com/{repository}/releases/download/{tag}"
    )
    # Platform-specific archive names end in arm64/x86_64, so Homebrew may
    # otherwise infer "64" as the formula version even though the release URL
    # contains the semantic tag. Keep the release version explicit for both
    # public and local preflight formulae.
    version_line = f'  version "{version}"\n'
    assets = CLIENT_ASSETS if package == "helixir-client" else SERVER_ASSETS

    def source(asset_key: str, indent: str = "    ") -> str:
        asset = assets[asset_key]
        return (
            f'{indent}url "{base}/{asset}"\n'
            f'{indent}sha256 "{checksums[asset]}"'
        )

    platform_sources = f'''  on_macos do
    on_arm do
{source("macos_arm", "      ")}
    end
    on_intel do
{source("macos_intel", "      ")}
    end
  end

  on_linux do
    on_arm do
{source("linux_arm", "      ")}
    end
    on_intel do
{source("linux_intel", "      ")}
    end
  end'''

    if package == "helixir-client":
        return f'''class HelixirClient < Formula
  desc "Thin remote-agent client for an existing Helixir MCP gateway"
  homepage "https://github.com/{repository}"
{version_line.rstrip()}
  license "MIT"

{platform_sources}

  def install
    libexec.install Dir["*"]
    bin.install_symlink libexec/"helixir-client"
  end

  def caveats
    <<~EOS
      Connect this agent-only host to an existing Helixir MCP gateway with:

        helixir-client connect --gateway HOST:8765 --principal ID --owner ID

      The client installs no database, models, daemon, or admin UI. Homebrew
      upgrades never remove ~/.helixir or managed agent instructions.
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{{bin}}/helixir-client --version")
    assert_match "connect", shell_output("#{{bin}}/helixir-client connect --help")
    assert_match "doctor", shell_output("#{{bin}}/helixir-client doctor --help")
    assert_path_exists libexec/"skills/helixir-memory/SKILL.md"
    assert_path_exists libexec/"integration/AGENTS.md"
    assert_path_exists libexec/"integration/SKILLS.md"
    refute_path_exists libexec/"helixir"
    refute_path_exists libexec/"helixir-mcp"
    refute_path_exists libexec/"schema"
  end
end
'''

    return f'''class Helixir < Formula
  desc "Graph-based persistent memory and reasoning layer for LLM agents"
  homepage "https://github.com/{repository}"
{version_line.rstrip()}
  license "MIT"

{platform_sources}

  def install
    libexec.install Dir["*"]
    %w[helixir helixir-mcp helixir-deploy].each do |command|
      bin.install_symlink libexec/command
    end
  end

  def caveats
    <<~EOS
      Finish installation and configure HelixDB, NLI, embeddings, MCP clients,
      RBAC and the optional web control plane with:

        helixir onboard

      Homebrew upgrades never remove ~/.helixir or external database volumes.
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{{bin}}/helixir --version")
    assert_match "onboard", shell_output("#{{bin}}/helixir onboard --help")
    assert_path_exists libexec/"schema/schema.hx"
    assert_path_exists libexec/"skills/helixir-memory/SKILL.md"
    assert_path_exists libexec/"integration/AGENTS.md"
    assert_path_exists libexec/"integration/SKILLS.md"
    refute_path_exists libexec/"helixir-client"
  end
end
'''


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tag", required=True)
    parser.add_argument("--checksums", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    parser.add_argument("--repository", default="nikita-rulenko/Helixir")
    parser.add_argument(
        "--package",
        choices=("helixir", "helixir-client"),
        default="helixir",
        help="formula to render (default: helixir)",
    )
    parser.add_argument(
        "--base-url",
        help="override immutable asset base (used by native pre-release tests)",
    )
    args = parser.parse_args()
    try:
        formula = render(
            args.tag,
            args.repository,
            parse_checksums(
                args.checksums,
                CLIENT_ASSETS if args.package == "helixir-client" else SERVER_ASSETS,
            ),
            args.package,
            args.base_url,
        )
    except (OSError, ValueError) as error:
        print(f"render_homebrew_formula: {error}", file=sys.stderr)
        return 2
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(formula, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
