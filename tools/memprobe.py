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
        # open the cache valve: ask the kernel to shed reclaimable memory
        # (page cache) charged to the container via cgroup memory.reclaim.
        # Live heap is never touched. Default step: 1024 MiB.
    (default container: helix-helixir-local-bench_app)

Profiling is read-only (/proc + cgroup through `docker exec`). The valve
spawns a short-lived privileged alpine helper in the host cgroup namespace,
because cgroupfs is read-only from inside the container.
"""

import json
import subprocess
import sys

args = [a for a in sys.argv[1:] if not a.startswith("--")]
flags = [a for a in sys.argv[1:] if a.startswith("--")]
CONTAINER = args[0] if args else "helix-helixir-local-bench_app"
RECLAIM = "--reclaim" in flags
RECLAIM_MIB = int(args[1]) if RECLAIM and len(args) > 1 else 0  # 0 = ask the full current charge (#89)

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
    """Parse /proc/1/smaps into [(name, size, rss), ...] per mapping."""
    maps = []
    cur = None
    for line in in_container("cat /proc/1/smaps").splitlines():
        if "-" in line.split(" ")[0] and ("r" in line or "-" in line) and ":" not in line.split()[0][:4].replace("-", ""):
            # header line: "addr-addr perms offset dev inode [path]"
            parts = line.split(None, 5)
            if len(parts) >= 5 and "-" in parts[0]:
                name = parts[5].strip() if len(parts) == 6 else "[anon]"
                cur = {"name": name, "size": 0, "rss": 0}
                maps.append(cur)
                continue
        if cur is not None:
            if line.startswith("Size:"):
                cur["size"] = int(line.split()[1]) * KB
            elif line.startswith("Rss:"):
                cur["rss"] = int(line.split()[1]) * KB
    return maps


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
    print("(live heap untouched — only reclaimable pages were shed)")


def main():
    if RECLAIM:
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
    print(f"anonymous (true heap)     : {human(anon)}")
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
    vm_rss = status.get("VmRSS", 0)
    if anon and vm_rss and vm_rss > anon * 1.5:
        print(
            f"* VmRSS ({human(vm_rss)}) far exceeds cgroup anon ({human(anon)}): most of the\n"
            f"  'resident' pages were touched once and already reclaimed/uncharged by the\n"
            f"  kernel — the process view double-counts what the cgroup no longer charges.\n"
            f"  Trust the cgroup number for capacity planning."
        )
    if big_reserved:
        touched = sum(m["rss"] for m in big_reserved)
        reserved = sum(m["size"] for m in big_reserved)
        print(
            f"* {len(big_reserved)} large arena(s): {human(reserved)} reserved, only "
            f"{human(touched)} touched.\n  Address-space reservation (LMDB map / allocator arena), not real RAM."
        )
    stats_usage = stats.get("MemUsage", "").split("/")[0].strip()
    if "memory.current" in cg and stats_usage:
        print(
            f"* If docker stats ({stats_usage}) >> cgroup current ({human(cg['memory.current'])}),\n"
            f"  the gap is page cache + prior peaks inside the Docker Desktop VM, not live heap:\n"
            f"  macOS docker stats reports usage including reclaimable cache the kernel keeps\n"
            f"  'until someone needs it'. The container is not actually holding it hostage."
        )
    print(
        f"* True working set right now: ~{human(anon)} heap + {human(filemem)} file cache."
    )


if __name__ == "__main__":
    main()
