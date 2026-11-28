# RE-23B: Linux environment

**Status: BLOCKED. No container measurement ran.** The Fedora machine is
available for builds and tests, but in its current state it does not
meet the soak plan's conditions for the benchmarks. The tools could not
reach its Docker daemon, and that daemon runs containers in a VM. The
images being compared were never moved to it.

Recorded 2026-11-28 from inside the VS Code Flatpak sandbox, the only
view of the host the tools had (see "What could not be recorded").

## The host

| Item | Value |
|---|---|
| Host name | `dennis-fedora` |
| CPU | Intel Core i5-9400F @ 2.90 GHz (max 4.1 GHz), 6 cores, 6 threads, 1 socket, microcode `0x104` |
| Physical RAM | 15.5 GiB (16 300 800 kB) |
| Kernel | `7.3.4-300.fc45.x86_64` (Fedora 45), `PREEMPT_DYNAMIC` |
| CPU governor | `performance` on all 6 cores |
| Swap | 8 GiB zram (`/dev/zram0`), **2.9 GiB in use** at the time of recording |
| Storage | 931.5 GiB rotational disk (`sda`, HDD); 763 GiB free on `/home` |
| Load (sandbox view) | 1.85 / 2.00 / 4.97 (1, 5, 15 min), 1 413 threads on the host |
| Pressure (PSI) | CPU some avg300 = 5.04 %; memory some avg300 = 0.34 %, full avg300 = 0.32 % |
| Docker | Client context `desktop-linux` (`~/.docker/desktop/docker.sock`): **Docker Desktop for Linux** |
| Rust | 1.98.0 (CI's pin) and 1.100.0 (local default), from `~/.cargo` |
| Node.js | 24.18.0 and 22.20.0 (CI's pin), official nodejs.org tarballs |

## Why this host does not yet qualify

The soak plan requires "a dedicated Linux machine (x86_64, cgroup v2,
nothing else running)". Four things stand in the way:

1. **Containers here run in a VM.** Docker Desktop for Linux runs the
   Docker engine inside its own KVM virtual machine. Containers inside it
   get that VM's cgroups and its vCPUs, not the host's. That is the same
   kind of virtualization layer RE-23B exists to remove: the Mac results
   were preliminary because they came from a Docker Desktop VM. Validation
   on dedicated Linux needs the native Docker Engine (`moby-engine` or
   `docker-ce`) or Podman with cgroup v2, running on the host kernel.
2. **No access from the tools.** The tools run in the VS Code Flatpak
   sandbox, which cannot see `/sys/fs/cgroup` or start host processes.
   The permission classifier denied the attempt to reach the host
   (`flatpak-spawn --host`). The Docker Desktop socket is visible in the
   home directory but was deliberately not used: it would have bypassed
   that decision, and its containers would run in the VM anyway.
3. **The machine is a busy workstation.** It runs a desktop session with
   about 1 400 threads, has 2.9 GiB swapped out to zram, and shows
   non-zero memory pressure. For the 6-hour soak and the p99 target, the
   desktop and other workloads must be stopped, and swap state must be
   recorded or cleared beforehand.
4. **The images are not on this host.** A search of the home directory
   found no `velasia-s1` image archive and no `base` or `re2c` binary.
   They exist only on the Mac where RE-2/3 ran.

## What could not be recorded

From the sandbox the tools could not record: the Docker server version,
cgroup version and driver as Docker sees them, the CPU and memory
available to containers, the host's process list, and any image hash.
These go in this report when the benchmarks run.

## Images to measure (from RE-2/3)

| Binary | sha256 | On this host |
|---|---|---|
| `base`, rc.3 | `af994c15e3c6d6a438cfece9e753cf651598f31b050dea40b693bc5ed06548a2` | no |
| `re2c`, final RE-2/3 | `052d741d4fc4659201da20679a9505e79e100f05dc51e9b5d1cdd25017191902` | no |
| `main` (`c65731b`, `re2c` + `cargo fmt`) | not built as an image | — |

Application: Velasia Airways `d04127a`. Node.js comparison: Next.js
16.4.6 standalone on Node.js 24.18.0.

## To unblock (owner)

1. Install the native Docker Engine (or Podman) on the Fedora host and
   confirm cgroup v2 (`docker info` → `Cgroup Version: 2`). Do not use
   Docker Desktop's `desktop-linux` context for measured runs.
2. Move the images with `docker save velasia-node:s1 velasia-nextrs:s1
   velasia-api:s1 | gzip > velasia-s1.tar.gz` on the Mac, then run
   `docker load` on this host and compare the binary hashes above. If
   the images are rebuilt instead, the reason and every binary
   difference must be recorded.
3. Run the benchmarks from a host terminal, not from the VS Code
   sandbox, or allow these tools to use host commands. The 6-hour runs
   also need the desktop session idle.
