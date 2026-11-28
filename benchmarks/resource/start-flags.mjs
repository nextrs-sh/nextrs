// The flags `nextrs start` accepts at a commit, read statically from the
// CLI's clap definitions (crates/nextrs-cli/src/main.rs): no build, no
// binary run. `run.mjs validate --commit LABEL=SHA` uses it to check that
// every variant's `args` exist in the binary that will serve them.
//
//   node benchmarks/resource/start-flags.mjs COMMIT|FILE     # JSON on stdout
//
// What it understands: the `Start { ... }` variant of `enum Command`, its
// `#[command(flatten)]` structs, and the global flags of `struct Cli`. A
// field with `#[arg(long)]` is `--field-name`; `long = "x"` is `--x`;
// `short` adds `-f`. A `bool` field takes no value; any other type takes
// one. It is a reader for this file's style, not a Rust parser: if it
// finds no `Start` variant it throws rather than guess.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const MAIN = "crates/nextrs-cli/src/main.rs";

/** The CLI source at a commit (git show, read-only) or from a file. */
export function cliSource(ref) {
  if (existsSync(ref)) return readFileSync(ref, "utf8");
  return execFileSync("git", ["-C", REPO, "show", `${ref}:${MAIN}`], { encoding: "utf8", maxBuffer: 64 << 20 });
}

/** The text between the `{` at or after `from` and its matching `}`. */
function block(src, from) {
  const open = src.indexOf("{", from);
  if (open < 0) return null;
  let depth = 0;
  for (let i = open; i < src.length; i++) {
    if (src[i] === "{") depth++;
    else if (src[i] === "}" && --depth === 0) return src.slice(open + 1, i);
  }
  return null;
}

/** Fields of a struct body or variant body, with the attributes before each. */
function fields(body) {
  const out = [];
  let attrs = [];
  let i = 0;
  while (i < body.length) {
    const rest = body.slice(i);
    const ws = rest.match(/^\s+/);
    if (ws) {
      i += ws[0].length;
      continue;
    }
    if (rest.startsWith("///") || rest.startsWith("//")) {
      i += rest.indexOf("\n") < 0 ? rest.length : rest.indexOf("\n") + 1;
      continue;
    }
    if (rest.startsWith("#[")) {
      let depth = 0;
      let j = 1;
      for (; j < rest.length; j++) {
        if (rest[j] === "[") depth++;
        else if (rest[j] === "]" && --depth === 0) break;
      }
      attrs.push(rest.slice(0, j + 1));
      i += j + 1;
      continue;
    }
    const m = rest.match(/^(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z0-9_]*)\s*:\s*/);
    if (!m) {
      i++;
      continue;
    }
    // The type runs to the next comma at depth 0.
    let j = m[0].length;
    let depth = 0;
    for (; j < rest.length; j++) {
      const c = rest[j];
      if (c === "<" || c === "(" || c === "[") depth++;
      else if (c === ">" || c === ")" || c === "]") depth--;
      else if (c === "," && depth === 0) break;
    }
    out.push({ name: m[1], type: rest.slice(m[0].length, j).trim(), attrs });
    attrs = [];
    i += j + 1;
  }
  return out;
}

const kebab = (s) => s.replace(/_/g, "-");

function flagsOf(fieldList, src, seen = new Set()) {
  const flags = {};
  for (const f of fieldList) {
    if (f.attrs.some((a) => /#\[command\(\s*flatten\s*\)\]/.test(a))) {
      const type = f.type.replace(/^Option<(.*)>$/, "$1");
      if (seen.has(type)) continue;
      seen.add(type);
      const at = src.search(new RegExp(`\\bstruct ${type}\\s*\\{`));
      if (at < 0) throw new Error(`flattened struct ${type} not found`);
      Object.assign(flags, flagsOf(fields(block(src, at)), src, seen));
      continue;
    }
    const arg = f.attrs.find((a) => a.startsWith("#[arg("));
    if (!arg) continue; // positional or subcommand
    const longNamed = arg.match(/\blong\s*=\s*"([^"]+)"/);
    const longBare = /\blong\b(?!\s*=)/.test(arg);
    const shortNamed = arg.match(/\bshort\s*=\s*'(.)'/);
    const shortBare = /\bshort\b(?!\s*=)/.test(arg);
    if (!longNamed && !longBare && !shortNamed && !shortBare) continue;
    const entry = {
      takesValue: f.type !== "bool",
      repeatable: /^Vec</.test(f.type),
      hyphenValues: /allow_hyphen_values\s*=\s*true/.test(arg),
      requires: arg.match(/\brequires\s*=\s*"([^"]+)"/)?.[1] ?? null,
    };
    if (longNamed || longBare) flags[`--${longNamed ? longNamed[1] : kebab(f.name)}`] = { ...entry, field: f.name };
    if (shortNamed || shortBare) flags[`-${shortNamed ? shortNamed[1] : f.name[0]}`] = { ...entry, field: f.name };
  }
  return flags;
}

/** `{ "--flag": { takesValue, repeatable, hyphenValues, requires, field } }` for `nextrs start`. */
export function startFlags(src) {
  const cmd = src.search(/\benum Command\s*\{/);
  if (cmd < 0) throw new Error("enum Command not found");
  const commandBody = block(src, cmd);
  const startAt = commandBody.search(/\bStart\s*\{/);
  if (startAt < 0) throw new Error("Command::Start not found");
  const flags = flagsOf(fields(block(commandBody, startAt)), src);
  const cli = src.search(/\bstruct Cli\s*\{/);
  if (cli >= 0) {
    // `struct Cli` holds the subcommand and the global flags (`--log-format`).
    for (const [k, v] of Object.entries(flagsOf(fields(block(src, cli)), src))) flags[k] ??= { ...v, global: true };
  }
  flags["--help"] ??= { takesValue: false, repeatable: false, hyphenValues: false, requires: null, field: "help" };
  return flags;
}

/**
 * Splits `args` as the images' start scripts do (`$NEXTRS_EXTRA_ARGS`,
 * unquoted: word splitting on whitespace, no quote handling) and checks
 * each word against `flags`. Returns a list of problems (empty: valid).
 */
export function checkArgs(args, flags) {
  const problems = [];
  const words = String(args ?? "").split(/\s+/).filter(Boolean);
  // Characters that would mean something to a shell or that the start
  // scripts' word splitting would not handle as intended.
  for (const w of words) {
    if (/["'`$\\;&|<>(){}*?\[\]~#]/.test(w)) problems.push(`"${w}": shell metacharacter (NEXTRS_EXTRA_ARGS is word-split, never quoted or expanded)`);
  }
  const given = new Set();
  for (let i = 0; i < words.length; i++) {
    const w = words[i];
    if (!w.startsWith("-")) {
      problems.push(`"${w}": a value without a flag`);
      continue;
    }
    const [name, inline] = w.includes("=") && w.startsWith("--") ? [w.slice(0, w.indexOf("=")), w.slice(w.indexOf("=") + 1)] : [w, undefined];
    const f = flags[name];
    if (!f) {
      problems.push(`${name}: not a \`nextrs start\` flag of this binary`);
      continue;
    }
    given.add(f.field);
    if (!f.takesValue) {
      if (inline !== undefined) problems.push(`${name}: takes no value`);
      continue;
    }
    if (inline !== undefined) continue;
    const v = words[i + 1];
    if (v === undefined || (v.startsWith("-") && !f.hyphenValues && !/^-\d/.test(v))) {
      problems.push(`${name}: needs a value`);
      continue;
    }
    i++;
  }
  for (const f of Object.values(flags)) {
    if (given.has(f.field) && f.requires && !given.has(f.requires)) {
      problems.push(`--${kebab(f.field)} requires --${kebab(f.requires)}`);
    }
  }
  return problems;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const ref = process.argv[2] ?? "HEAD";
  console.log(JSON.stringify(startFlags(cliSource(ref)), null, 1));
}
