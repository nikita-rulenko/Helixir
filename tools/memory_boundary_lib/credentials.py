"""Private LLM credential resolution for faithful memory-boundary runs."""

from __future__ import annotations

import os
import tomllib
from pathlib import Path
from typing import Any

from .contract import (
    FAITHFUL_LLM_MODEL,
    FAITHFUL_LLM_PROVIDER,
    HarnessError,
    ROOT,
)


def private_llm_env(scenario: dict[str, Any]) -> dict[str, str]:
    """Load required auth from a private TOML without serializing the secret."""
    runtime = scenario.get("llm_runtime")
    if runtime is None:
        return {}
    source_raw = os.environ.get("HELIXIR_MEMORY_HARNESS_LLM_CONFIG", "").strip()
    if not source_raw:
        raise HarnessError(
            "HELIXIR_MEMORY_HARNESS_LLM_CONFIG is required for llm_runtime"
        )
    source = Path(source_raw).expanduser().resolve()
    if source == ROOT or ROOT in source.parents:
        raise HarnessError("private LLM config must stay outside the repository")
    if not source.is_file():
        raise HarnessError("private LLM config is not a regular file")
    if source.stat().st_mode & 0o077:
        raise HarnessError("private LLM config permissions must be 0600 or stricter")
    try:
        with source.open("rb") as stream:
            config = tomllib.load(stream)
    except tomllib.TOMLDecodeError as error:
        raise HarnessError("private LLM config is not valid TOML") from error
    provider = runtime["provider"]
    model = runtime["model"]
    if provider != FAITHFUL_LLM_PROVIDER or model != FAITHFUL_LLM_MODEL:
        raise HarnessError("faithful LLM runtime is not pinned to cerebras/gpt-oss")
    if config.get("llm_provider") != provider or config.get("llm_model") != model:
        raise HarnessError("private LLM config does not match the pinned runtime")
    api_key = config.get("llm_api_key")
    if not isinstance(api_key, str) or not api_key.strip():
        raise HarnessError("private LLM config has no non-empty llm_api_key")
    return {
        "HELIX_LLM_PROVIDER": provider,
        "HELIX_LLM_MODEL": model,
        "HELIX_LLM_API_KEY": api_key,
    }
