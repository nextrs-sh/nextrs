#!/usr/bin/env python3
"""Summaries and a flamegraph from a folded-stack profile (profile.mjs).

    python3 benchmarks/phase3a/analyze_profile.py PROFILE.folded [--svg OUT.svg] [--json OUT.json]

Prints: samples per thread, self time by category, the top self frames, and
inclusive time of NextRS's own Rust functions and JavaScript polyfills (a
frame counted once per sample even when recursive). Stdlib only.
"""
import argparse
import html
import json
import re
import sys
from collections import Counter, defaultdict

CATEGORIES = [
    ("v8-gc", re.compile(r"v8::internal::(Scavenger|Scavenge|MarkCompact|ConcurrentMarking|Marking|Sweeper|ArrayBufferSweeper|Heap::|MinorMark|RecordMigrated|BodyDescriptorBase|HeapObject::SizeFromMap|MainMarking|EphemeronTable|IncrementalMarking|MemoryAllocator|PagedSpace|LargeObjectSpace|cppgc)|ScavengerPromoted|MarkingVisitor|Builtins_RecordWrite")),
    ("v8-compiler", re.compile(r"v8::internal::(compiler|maglev|baseline|Compiler|Parser|PreParser|Scanner|SourcePosition|Bytecode|interpreter::BytecodeGenerator|AstNode|Deoptimizer|turboshaft|wasm)")),
    ("nextrs-js-polyfill", re.compile(r"^JS:.*ext:nextrs/")),
    ("deno-core-js", re.compile(r"^JS:.*ext:(core|deno_)")),
    ("next-react-js", re.compile(r"^JS:")),
    ("v8-builtins", re.compile(r"^Builtins_|^Stub:|^BytecodeHandler:|^Builtin:|^RegExp:")),
    ("v8-runtime", re.compile(r"v8::|^Runtime_|v8_|^Isolate")),
    ("allocator", re.compile(r"malloc|_int_free|cfree|^free$|realloc|calloc|unlink_chunk|consolidate|tcache|arena|sysmalloc|__libc_|mmap|munmap|brk")),
    ("memcpy/memset", re.compile(r"mem(move|cpy|set|cmp)|__memchr|strlen")),
    ("rust-nextrs", re.compile(r"nextrs_(server|runtime|adapter|image|cli)")),
    ("rust-deno-core", re.compile(r"deno_core|serde_v8|deno_")),
    ("rust-http", re.compile(r"hyper|axum|^http::|tower|h2::|httparse|^bytes::")),
    ("rust-tokio", re.compile(r"tokio|mio::|futures_")),
    ("rust-json/regex", re.compile(r"serde_json|regex|fancy_regex|aho_corasick")),
    ("rust-image", re.compile(r"image::|zune|webp|mozjpeg|png::|rav1e|ravif|fast_image_resize|jpeg")),
    ("rust-other", re.compile(r"::|^core::|^alloc::|^std::")),
    ("libc-other", re.compile(r"^__|@GLIBC|pthread|futex|epoll|syscall|libc|^_dl|clock_gettime|write|read|sendto|recv")),
]
UNKNOWN = re.compile(r"^\[")


def category(frame):
    if UNKNOWN.match(frame):
        return "unknown (" + frame.strip("[]") + ")"
    for name, rx in CATEGORIES:
        if rx.search(frame):
            return name
    return "other"


def thread_group(name):
    name = re.sub(r"[-_ ]?\d+$", "", name)
    if name.startswith("V8 ") or name == "V8":
        return "v8-platform-workers"
    if name.startswith("nextrs-v8") or name.startswith("nextrs-v"):
        return "isolate (JS) threads"
    if name.startswith("tokio"):
        return "tokio workers"
    return name


def short(sym):
    sym = re.sub(r"<[^<>]*>", "", sym)
    sym = re.sub(r"<[^<>]*>", "", sym)
    sym = re.sub(r"\(.*\)( const)?$", "", sym)
    sym = re.sub(r"::h[0-9a-f]{16}$", "", sym)
    sym = re.sub(r" file:///\S*/(fixtures/[^/]+/)", r" \1", sym)
    return sym.strip()


def load(path):
    stacks = []
    for line in open(path, encoding="utf-8", errors="replace"):
        line = line.rstrip("\n")
        if not line:
            continue
        stack, _, n = line.rpartition(" ")
        frames = stack.split(";")
        stacks.append((frames[0], frames[1:], int(n)))
    return stacks


def summarize(stacks):
    total = sum(n for _, _, n in stacks)
    threads = Counter()
    self_cat = Counter()
    self_sym = Counter()
    incl = Counter()
    for thread, frames, n in stacks:
        threads[thread_group(thread)] += n
        leaf = frames[-1] if frames else "[none]"
        self_cat[category(leaf)] += n
        self_sym[short(leaf)] += n
        seen = set()
        for f in frames:
            s = short(f)
            if s in seen:
                continue
            seen.add(s)
            if re.search(r"nextrs_(server|runtime|adapter|image|cli)::|ext:nextrs/|deno_core::|serde_v8::", f):
                incl[s] += n
    return total, threads, self_cat, self_sym, incl


def pct(n, total):
    return f"{100 * n / total:6.2f}%"


def flamegraph(stacks, out, title):
    root = {"n": 0, "c": {}}
    for thread, frames, n in stacks:
        node = root
        node["n"] += n
        for f in [thread_group(thread)] + [short(x) for x in frames]:
            node = node["c"].setdefault(f, {"n": 0, "c": {}})
            node["n"] += n
    width, row = 1600, 16
    rects = []
    depth_max = [0]

    def walk(name, node, x, depth):
        w = node["n"] / root["n"] * width
        if w < 0.3:
            return
        depth_max[0] = max(depth_max[0], depth)
        cat = category(name) if depth > 1 else "thread"
        rects.append((x, depth, w, name, node["n"], cat))
        cx = x
        for k, child in sorted(node["c"].items()):
            walk(k, child, cx, depth + 1)
            cx += child["n"] / root["n"] * width

    walk("all", root, 0, 0)
    palette = {
        "v8-gc": "#e45756", "v8-compiler": "#f58518", "nextrs-js-polyfill": "#b279a2", "deno-core-js": "#9d755d",
        "next-react-js": "#4c78a8", "v8-builtins": "#72b7b2", "v8-runtime": "#54a24b", "allocator": "#ff9da6",
        "memcpy/memset": "#eeca3b", "rust-nextrs": "#d67195", "rust-deno-core": "#bab0ac", "rust-http": "#79706e",
        "rust-tokio": "#9ecae9", "rust-json/regex": "#fdae6b", "rust-image": "#88d27a", "thread": "#cccccc",
    }
    height = (depth_max[0] + 3) * row
    parts = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" font-family="monospace" font-size="11">',
             f'<rect width="100%" height="100%" fill="#ffffff"/><text x="4" y="12">{html.escape(title)} — {root["n"]} samples; width = share of samples (icicle: callers on top)</text>']
    for x, depth, w, name, n, cat in rects:
        y = (depth + 1) * row
        color = palette.get(cat, "#c7c7c7")
        label = html.escape(name if w > 7 * len(name) else name[: max(0, int(w / 7) - 2)] + ("…" if w > 20 else ""))
        tip = html.escape(f"{name} — {n} samples ({100 * n / root['n']:.2f}%) [{cat}]")
        parts.append(f'<g><title>{tip}</title><rect x="{x:.2f}" y="{y}" width="{max(w - 0.5, 0.1):.2f}" height="{row - 1}" fill="{color}"/>'
                     + (f'<text x="{x + 2:.2f}" y="{y + 11}">{label}</text>' if w > 20 else "") + "</g>")
    parts.append("</svg>")
    with open(out, "w") as f:
        f.write("\n".join(parts))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("folded")
    ap.add_argument("--svg")
    ap.add_argument("--json")
    ap.add_argument("--top", type=int, default=30)
    a = ap.parse_args()
    stacks = load(a.folded)
    total, threads, self_cat, self_sym, incl = summarize(stacks)
    print(f"{a.folded}: {total} samples")
    print("\n# samples by thread")
    for k, v in threads.most_common():
        print(f"{pct(v, total)}  {k}")
    print("\n# self time by category")
    for k, v in self_cat.most_common():
        print(f"{pct(v, total)}  {k}")
    print(f"\n# top {a.top} self frames")
    for k, v in self_sym.most_common(a.top):
        print(f"{pct(v, total)}  {k[:150]}")
    print(f"\n# inclusive: NextRS Rust, polyfills, deno_core (top {a.top})")
    for k, v in incl.most_common(a.top):
        print(f"{pct(v, total)}  {k[:150]}")
    if a.json:
        with open(a.json, "w") as f:
            json.dump({"samples": total, "threads": threads, "self_categories": self_cat, "top_self": self_sym.most_common(100), "inclusive": incl.most_common(200)}, f, indent=1)
    if a.svg:
        flamegraph(stacks, a.svg, a.folded.rsplit("/", 1)[-1])


if __name__ == "__main__":
    sys.exit(main())
