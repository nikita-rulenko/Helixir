#!/usr/bin/env python3
"""memprobe — where did the container's RAM actually go? (#89)

Takes a live memory profile of a running HelixDB container and reconciles
the three numbers that never match: `docker stats`, the cgroup accounting,
and the process view (/proc/1). Then it walks /proc/1/smaps and classifies
every mapping, so the verdict names WHAT is resident — live heap, reserved
arenas, file maps — instead of one scary total.

Usage:
    python3 tools/memprobe.py [container-name]            # profile (read-only)
    python3 tools/memprobe.py [container-name] --reclaim [MiB]
        # ask the kernel to shed reclaimable memory charged to the container.
        # The default request covers the full current charge; allocated or
        # allocator-retained anonymous pages are not reclaimable this way.
    python3 tools/memprobe.py [container-name] --dump-to DIR
        # stream large anonymous mappings through zstd into a private directory.
        # WARNING: dumps contain application data and are not atomic.
    python3 tools/memprobe.py --analyze-dump DIR
        # classify a prior dump without printing recovered application content.
    (default container: helix-helixir-local-bench_app)

Profiling is read-only (/proc + cgroup through `docker exec`). The valve
spawns a short-lived privileged alpine helper in the host cgroup namespace,
because cgroupfs is read-only from inside the container.
"""

import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys


def parse_args():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("container", nargs="?", default="helix-helixir-local-bench_app")
    parser.add_argument(
        "--reclaim",
        nargs="?",
        const=0,
        type=int,
        metavar="MIB",
        help="ask cgroup v2 to reclaim memory (default: full current charge)",
    )
    parser.add_argument("--dump-to", type=Path, metavar="DIR")
    parser.add_argument("--analyze-dump", type=Path, metavar="DIR")
    return parser.parse_args()


ARGS = parse_args()
CONTAINER = ARGS.container
RECLAIM_MIB = ARGS.reclaim

KB = 1024
MB = 1024 * 1024


def sh(cmd):
    return subprocess.run(cmd, capture_output=True, text=True).stdout


def in_container(shell_cmd):
    return sh(["docker", "exec", CONTAINER, "sh", "-c", shell_cmd])


def human(nbytes):
    if nbytes >= 1024 * MB:
        return f"{nbytes / (1024 * MB):.2f}GiB"
    if nbytes >= MB:
        return f"{nbytes / MB:.0f}MiB"
    return f"{nbytes / KB:.0f}KiB"


# ---------------------------------------------------------------- collectors

def docker_stats():
    raw = sh(["docker", "stats", "--no-stream", "--format", "{{json .}}", CONTAINER])
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        return {}


def proc_status():
    out = {}
    for line in in_container("cat /proc/1/status").splitlines():
        if line.startswith(("VmRSS", "VmSize", "RssAnon", "RssFile", "RssShmem", "VmSwap")):
            k, v = line.split(":", 1)
            out[k] = int(v.strip().split()[0]) * KB
    return out


def cgroup():
    out = {}
    cur = in_container("cat /sys/fs/cgroup/memory.current 2>/dev/null").strip()
    if cur.isdigit():
        out["memory.current"] = int(cur)
    for line in in_container("cat /sys/fs/cgroup/memory.stat 2>/dev/null").splitlines():
        parts = line.split()
        if len(parts) == 2 and parts[1].isdigit():
            out[parts[0]] = int(parts[1])
    return out


def smaps():
    """Parse /proc/1/smaps into one dictionary per mapping."""
    maps = []
    cur = None
    for line in in_container("cat /proc/1/smaps").splitlines():
        if "-" in line.split(" ")[0] and ("r" in line or "-" in line) and ":" not in line.split()[0][:4].replace("-", ""):
            # header line: "addr-addr perms offset dev inode [path]"
            parts = line.split(None, 5)
            if len(parts) >= 5 and "-" in parts[0]:
                start, end = (int(value, 16) for value in parts[0].split("-", 1))
                name = parts[5].strip() if len(parts) == 6 else "[anon]"
                cur = {
                    "name": name,
                    "start": start,
                    "end": end,
                    "perms": parts[1],
                    "size": 0,
                    "rss": 0,
                    "anonymous": 0,
                    "private_dirty": 0,
                    "lazy_free": 0,
                }
                maps.append(cur)
                continue
        if cur is not None:
            if line.startswith("Size:"):
                cur["size"] = int(line.split()[1]) * KB
            elif line.startswith("Rss:"):
                cur["rss"] = int(line.split()[1]) * KB
            elif line.startswith("Anonymous:"):
                cur["anonymous"] = int(line.split()[1]) * KB
            elif line.startswith("Private_Dirty:"):
                cur["private_dirty"] = int(line.split()[1]) * KB
            elif line.startswith("LazyFree:"):
                cur["lazy_free"] = int(line.split()[1]) * KB
    return maps


def secure_json_dump(path, value):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as out:
        json.dump(value, out, indent=2)
        out.write("\n")


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while block := source.read(8 * MB):
            digest.update(block)
    return digest.hexdigest()


def dump_heap(output_dir):
    """Stream large writable anonymous mappings to private zstd files."""
    output_dir.mkdir(mode=0o700, parents=True, exist_ok=False)
    selected = [
        mapping
        for mapping in smaps()
        if mapping["name"] == "[anon]"
        and "w" in mapping["perms"]
        and mapping["rss"] >= 32 * MB
    ]
    if not selected:
        print("no writable anonymous mapping with at least 32MiB RSS found")
        return 1

    manifest = {
        "container": CONTAINER,
        "created_at": datetime.now(timezone.utc).isoformat(),
        "warning": "Contains private process memory. Keep mode 0600 and never commit or upload.",
        "atomic": False,
        "mappings": [],
    }
    print("WARNING: heap dumps contain application data; files are mode 0600.")
    print("The process remains live, so mappings are sampled sequentially, not atomically.")
    for mapping in sorted(selected, key=lambda item: item["rss"], reverse=True):
        name = f"arena-{mapping['start']:x}-{mapping['end']:x}.bin.zst"
        path = output_dir / name
        dd = subprocess.Popen(
            [
                "docker", "exec", CONTAINER, "dd", "if=/proc/1/mem",
                "iflag=skip_bytes,count_bytes", f"skip={mapping['start']}",
                f"count={mapping['size']}", "status=none",
            ],
            stdout=subprocess.PIPE,
        )
        zstd = subprocess.run(
            ["zstd", "-T0", "-1", "-q", "-o", str(path)],
            stdin=dd.stdout,
        )
        if dd.stdout is not None:
            dd.stdout.close()
        dd_code = dd.wait()
        if dd_code != 0 or zstd.returncode != 0:
            path.unlink(missing_ok=True)
            raise RuntimeError(f"dump failed for {mapping['start']:x}-{mapping['end']:x}")
        path.chmod(0o600)
        record = {**mapping, "file": name, "sha256": sha256_file(path)}
        manifest["mappings"].append(record)
        print(f"dumped {human(mapping['size'])} ({human(mapping['rss'])} RSS) -> {path}")
    secure_json_dump(output_dir / "manifest.json", manifest)
    print(f"manifest: {output_dir / 'manifest.json'}")
    return 0


ID_PATTERN = re.compile(rb"(?:mem|raw)_[0-9a-f]{12}")
NON_PRINTABLE = bytes(
    byte for byte in range(256) if byte not in (9, 10, 13) and not 32 <= byte <= 126
)
ZERO_PAGE = bytes(4096)
STRUCTURAL_MARKERS = {
    "memory_ids": ID_PATTERN,
    "json_content_keys": re.compile(rb'\"content\"\s*:'),
    "json_user_id_keys": re.compile(rb'\"user_id\"\s*:'),
    "json_created_at_keys": re.compile(rb'\"created_at\"\s*:'),
    "rfc3339_timestamps": re.compile(rb"20[0-9]{2}-[01][0-9]-[0-3][0-9]T[0-9]{2}:[0-9]{2}:[0-9]{2}"),
}


def analyze_file(path):
    """Classify dump bytes without exposing any recovered strings."""
    proc = subprocess.Popen(["zstd", "-dc", str(path)], stdout=subprocess.PIPE)
    totals = Counter()
    ids = Counter()
    carry = b""
    while proc.stdout is not None and (chunk := proc.stdout.read(8 * MB)):
        totals["bytes"] += len(chunk)
        totals["zero_bytes"] += chunk.count(0)
        totals["printable_bytes"] += len(chunk.translate(None, NON_PRINTABLE))
        view = memoryview(chunk)
        for offset in range(0, len(chunk), 4096):
            totals["pages"] += 1
            if view[offset:offset + 4096] == ZERO_PAGE:
                totals["zero_pages"] += 1
        searchable = carry + chunk
        for marker, pattern in STRUCTURAL_MARKERS.items():
            matches = [
                match.group(0)
                for match in pattern.finditer(searchable)
                if match.end() > len(carry)
            ]
            totals[marker] += len(matches)
            if marker == "memory_ids":
                ids.update(matches)
        carry = searchable[-64:]
    if proc.wait() != 0:
        raise RuntimeError(f"zstd failed while reading {path}")
    repeated = sum(1 for count in ids.values() if count > 1)
    totals["unique_memory_ids"] = len(ids)
    totals["repeated_memory_ids"] = repeated
    totals["memory_ids_once"] = sum(1 for count in ids.values() if count == 1)
    totals["memory_ids_10_plus"] = sum(1 for count in ids.values() if count >= 10)
    totals["memory_ids_100_plus"] = sum(1 for count in ids.values() if count >= 100)
    totals["max_memory_id_copies"] = max(ids.values(), default=0)
    return totals


def analyze_dump(directory):
    files = sorted(directory.glob("arena-*.bin.zst"))
    if not files:
        print(f"no arena-*.bin.zst files found in {directory}")
        return 1
    for path in files:
        totals = analyze_file(path)
        size = totals["bytes"] or 1
        pages = totals["pages"] or 1
        print(f"== {path.name} ==")
        print(f"decoded bytes       : {human(totals['bytes'])}")
        print(f"zero bytes/pages    : {100 * totals['zero_bytes'] / size:.1f}% / {100 * totals['zero_pages'] / pages:.1f}%")
        print(f"printable bytes     : {100 * totals['printable_bytes'] / size:.1f}%")
        print(f"memory ids          : {totals['memory_ids']} occurrences, {totals['unique_memory_ids']} unique, {totals['repeated_memory_ids']} repeated")
        print(f"ID copy histogram   : once={totals['memory_ids_once']}, >=10={totals['memory_ids_10_plus']}, >=100={totals['memory_ids_100_plus']}, max={totals['max_memory_id_copies']}")
        print(f"JSON content keys   : {totals['json_content_keys']}")
        print(f"JSON user_id keys   : {totals['json_user_id_keys']}")
        print(f"JSON created_at keys: {totals['json_created_at_keys']}")
        print(f"RFC3339 timestamps  : {totals['rfc3339_timestamps']}")
    return 0


# ------------------------------------------------------------------ analysis

def reclaim():
    cid = sh(["docker", "inspect", "-f", "{{.Id}}", CONTAINER]).strip()
    if not cid:
        print(f"container '{CONTAINER}' not found")
        sys.exit(1)
    before = cgroup().get("memory.current", 0)
    ask_mib = RECLAIM_MIB if RECLAIM_MIB > 0 else max(1024, int(before) // 1048576 + 64)
    script = (
        f"for p in /sys/fs/cgroup/docker/{cid} /sys/fs/cgroup/system.slice/docker-{cid}.scope; do "
        f'if [ -f "$p/memory.reclaim" ]; then echo {ask_mib}M > "$p/memory.reclaim" || true; exit 0; fi; '
        f"done; exit 1"
    )
    r = subprocess.run(
        ["docker", "run", "--rm", "--privileged", "--pid=host", "--cgroupns=host",
         "alpine", "sh", "-c", script],
        capture_output=True, text=True,
    )
    if r.returncode != 0:
        print("reclaim failed: no memory.reclaim found in either cgroup layout")
        sys.exit(1)
    after = cgroup().get("memory.current", 0)
    print(f"cache valve: asked {ask_mib}MiB, charge {human(before)} -> {human(after)}")
    print("(only kernel-reclaimable pages were shed; allocator-retained heap is unchanged)")


def main():
    if ARGS.analyze_dump is not None:
        raise SystemExit(analyze_dump(ARGS.analyze_dump))
    if ARGS.dump_to is not None:
        raise SystemExit(dump_heap(ARGS.dump_to))
    if RECLAIM_MIB is not None:
        reclaim()
        return
    stats = docker_stats()
    status = proc_status()
    cg = cgroup()
    maps = smaps()

    print(f"== memprobe: {CONTAINER} ==\n")

    print("-- the three views of one process --")
    print(f"docker stats     : {stats.get('MemUsage', 'n/a')}  (what the dashboard screams)")
    if "memory.current" in cg:
        print(f"cgroup current   : {human(cg['memory.current'])}  (what the kernel charges the container)")
    print(f"process VmRSS    : {human(status.get('VmRSS', 0))}  (pages resident for pid 1)")
    print(f"  of it RssAnon  : {human(status.get('RssAnon', 0))}  (heap/arenas)")
    print(f"  of it RssFile  : {human(status.get('RssFile', 0))}  (binaries + mmapped files, incl. LMDB)")
    print(f"process VmSize   : {human(status.get('VmSize', 0))}  (address space RESERVED, mostly not real)")

    anon = cg.get("anon", 0) or cg.get("active_anon", 0) + cg.get("inactive_anon", 0)
    filemem = cg.get("file", 0) or cg.get("active_file", 0) + cg.get("inactive_file", 0)
    print("\n-- cgroup breakdown --")
    print(f"anonymous resident charge : {human(anon)}")
    print("  (live allocations + freed pages retained by the allocator)")
    print(f"file-backed (reclaimable) : {human(filemem)}")
    print(f"slab (kernel)             : {human(cg.get('slab', 0))}")

    # mapping classification
    anon_rss = sum(m["rss"] for m in maps if m["name"] == "[anon]")
    anon_reserved = sum(m["size"] for m in maps if m["name"] == "[anon]")
    file_rss = sum(m["rss"] for m in maps if m["name"].startswith("/"))
    big_reserved = [m for m in maps if m["name"] == "[anon]" and m["size"] >= 256 * MB]
    big_resident = sorted(
        (m for m in maps if m["rss"] >= 32 * MB),
        key=lambda m: -m["rss"],
    )

    print("\n-- /proc/1/smaps: what the mappings say --")
    print(f"anon mappings: reserved {human(anon_reserved)}, resident {human(anon_rss)}")
    print(f"anon LazyFree: {human(sum(m['lazy_free'] for m in maps))}")
    print(f"file mappings resident: {human(file_rss)}")
    if big_reserved:
        print(f"\nlarge anon arenas (>=256MiB reserved) — allocator/runtime reservations:")
        for m in big_reserved:
            pct = 100 * m["rss"] / m["size"] if m["size"] else 0
            print(f"  reserved {human(m['size']):>9}  resident {human(m['rss']):>9}  ({pct:.0f}% touched)")
    if big_resident:
        print(f"\nheaviest resident mappings (>=32MiB Rss):")
        for m in big_resident[:10]:
            print(f"  {human(m['rss']):>9}  {m['name']}")

    # ----------------------------------------------------------- the verdict
    print("\n== VERDICT ==")
    if big_reserved:
        touched = sum(m["rss"] for m in big_reserved)
        reserved = sum(m["size"] for m in big_reserved)
        print(
            f"* {len(big_reserved)} large arena(s): {human(reserved)} reserved, only "
            f"{human(touched)} resident.\n  Untouched address space is free, but the resident portion is charged and may\n"
            f"  contain either live allocations or freed pages retained by the allocator."
        )
    print(
        f"* Current charge: ~{human(anon)} anonymous + {human(filemem)} file-backed.\n"
        f"  Use LazyFree and a before/after workload test to distinguish live objects\n"
        f"  from allocator high-water retention; RSS alone cannot do that."
    )


if __name__ == "__main__":
    main()
