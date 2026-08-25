#!/usr/bin/env python3
"""Run or replay a fail-closed differential Helixir memory trace."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

if __package__:
    from .memory_boundary_lib import (
        HarnessError,
        ROOT,
        canonical_verdict,
        command_exit_code,
        configured_limit_metadata,
        docker_env,
        load_json,
        preflight,
        private_metadata,
        reset_target,
        run_metadata,
        run_scenario,
        trace_digest,
        validate_trace,
        write_reports,
    )
else:
    from memory_boundary_lib import (
        HarnessError,
        ROOT,
        canonical_verdict,
        command_exit_code,
        configured_limit_metadata,
        docker_env,
        load_json,
        preflight,
        private_metadata,
        reset_target,
        run_metadata,
        run_scenario,
        trace_digest,
        validate_trace,
        write_reports,
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--trace", type=Path)
    source.add_argument("--replay", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--dry-run", action="store_true")
    return parser.parse_args()


def load_trace(args: argparse.Namespace) -> dict:
    if not args.replay:
        return load_json(args.trace)
    prior = load_json(args.replay)
    trace = prior.get("trace")
    if not isinstance(trace, dict) or prior.get("trace_digest") != trace_digest(trace):
        raise HarnessError("replay report has no intact embedded trace")
    return trace


def execute(trace: dict, output: Path) -> tuple[dict, int]:
    preflight(trace)
    results = []
    started_by_target: dict[str, str] = {}
    for scenario in trace["scenarios"]:
        try:
            result, started = run_scenario(
                trace, scenario, output, started_by_target.get(scenario["target"])
            )
        except Exception as error:
            target = trace["targets"][scenario["target"]]
            try:
                reset_target(target, docker_env(trace), None)
            except Exception as cleanup_error:
                raise HarnessError(
                    f"scenario failed ({error}); clean restart also failed ({cleanup_error})"
                ) from cleanup_error
            raise
        started_by_target[scenario["target"]] = started
        results.append(result)
        if result["aborted"]:
            break
    report = {
        "schema_version": trace["schema_version"],
        "trace_digest": trace_digest(trace),
        "trace": trace,
        "run_metadata": run_metadata(trace),
        "canonical_verdict": canonical_verdict(results),
        "configured_limits": configured_limit_metadata(results),
        "results": results,
        "private_artifacts": private_metadata(
            output, trace.get("private_artifacts", [])
        ),
    }
    write_reports(output, report)
    return report, command_exit_code(results)


def main() -> int:
    args = parse_args()
    try:
        trace = load_trace(args)
        validate_trace(trace)
        if args.dry_run:
            print(
                json.dumps(
                    {
                        "dry_run": True,
                        "trace_digest": trace_digest(trace),
                        "run_metadata": run_metadata(trace),
                        "scenarios": [
                            {
                                "name": row["name"],
                                "target": row["target"],
                                "command": row["command"],
                            }
                            for row in trace["scenarios"]
                        ],
                    },
                    indent=2,
                )
            )
            return 0
        if args.output is None:
            raise HarnessError("live run requires --output outside the repository")
        output = args.output.expanduser().resolve()
        if output == ROOT or ROOT in output.parents:
            raise HarnessError("output must stay outside the repository")
        output.mkdir(mode=0o700, parents=True, exist_ok=False)
        _, exit_code = execute(trace, output)
        return exit_code
    except (HarnessError, OSError, ValueError, json.JSONDecodeError) as error:
        print(f"memory-boundary: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
