#!/usr/bin/env python3
"""Render the Helixir Homebrew formula from immutable release checksums."""

from __future__ import annotations

import argparse
import pathlib
import re
import sys


ASSETS = {
    "macos_arm": "helixir-macos-arm64.tar.gz",
    "macos_intel": "helixir-macos-x86_64.tar.gz",
    "linux_arm": "helixir-linux-arm64.tar.gz",
    "linux_intel": "helixir-linux-x86_64.tar.gz",
}


def parse_checksums(path: pathlib.Path) -> dict[str, str]:
    checksums: dict[str, str] = {}
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        parts = line.split()
        if len(parts) != 2 or not re.fullmatch(r"[0-9a-fA-F]{64}", parts[0]):
            raise ValueError(f"invalid checksum line {number}: {line!r}")
        checksums[pathlib.Path(parts[1].lstrip("*")).name] = parts[0].lower()
    missing = sorted(set(ASSETS.values()) - checksums.keys())
    if missing:
        raise ValueError(f"missing release checksums: {', '.join(missing)}")
    return checksums


def render(tag: str, repository: str, checksums: dict[str, str]) -> str:
    if not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?", tag):
        raise ValueError(f"invalid release tag: {tag!r}")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError(f"invalid GitHub repository: {repository!r}")
    version = tag.removeprefix("v")
    base = f"https://github.com/{repository}/releases/download/{tag}"

    def source(asset_key: str, indent: str = "    ") -> str:
        asset = ASSETS[asset_key]
        return (
            f'{indent}url "{base}/{asset}"\n'
            f'{indent}sha256 "{checksums[asset]}"'
        )

    return f'''class Helixir < Formula
  desc "Graph-based persistent memory and reasoning layer for LLM agents"
  homepage "https://github.com/{repository}"
  version "{version}"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
{source("macos_arm", "      ")}
    else
{source("macos_intel", "      ")}
    end
  end

  on_linux do
    if Hardware::CPU.arm?
{source("linux_arm", "      ")}
    else
{source("linux_intel", "      ")}
    end
  end

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
  end
end
'''


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tag", required=True)
    parser.add_argument("--checksums", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    parser.add_argument("--repository", default="nikita-rulenko/Helixir")
    args = parser.parse_args()
    try:
        formula = render(args.tag, args.repository, parse_checksums(args.checksums))
    except (OSError, ValueError) as error:
        print(f"render_homebrew_formula: {error}", file=sys.stderr)
        return 2
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(formula, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
