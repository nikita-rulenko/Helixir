"""Public surface for the memory-boundary CLI and model-free tests."""

from .contract import (
    HarnessError,
    ROOT,
    docker_env,
    load_json,
    trace_digest,
    validate_trace,
)
from .reporting import (
    canonical_verdict,
    collect_profiler_artifacts,
    command_exit_code,
    configured_limit_metadata,
    private_metadata,
    run_metadata,
    write_reports,
)
from .runtime import (
    container_memory_limit,
    guard_reason,
    preflight,
    process_memory,
    reset_target,
    run_scenario,
    scenario_env,
    workload_identity,
)

__all__ = [
    "HarnessError",
    "ROOT",
    "canonical_verdict",
    "collect_profiler_artifacts",
    "command_exit_code",
    "configured_limit_metadata",
    "container_memory_limit",
    "docker_env",
    "guard_reason",
    "load_json",
    "preflight",
    "private_metadata",
    "process_memory",
    "reset_target",
    "run_metadata",
    "run_scenario",
    "scenario_env",
    "trace_digest",
    "validate_trace",
    "workload_identity",
    "write_reports",
]
