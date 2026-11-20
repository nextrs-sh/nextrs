# Vendored Deno extension sources

These are unmodified copies of the JavaScript sources in two pinned crates:

| Directory | Source |
|---|---|
| `deno_web/` | `deno_web` 0.291.0, every `*.js` file at the crate root |
| `deno_webidl/` | `deno_webidl` 0.260.0, `00_webidl.js` |

## Why they are vendored

`deno_core` 0.413.0 records an extension's `lazy_loaded_js` and
`lazy_loaded_esm` files by their absolute path on the build machine. It
expects those files to be read once, while a startup snapshot is built.
NextRS uses no snapshot, so the path is read every time an isolate starts.
A binary built that way runs only on a machine that has the same Cargo
registry at the same path.

`src/embedded_js.rs` compiles these copies into the binary, and swaps them
in for the path-based entries before any runtime is created.

## Keeping them in step

The `embedded_js` unit tests check two things:

- each copy is byte-for-byte equal to the pinned crate's file;
- after the swap, no file in either extension is still loaded from a path.

So when `deno_web` or `deno_webidl` is upgraded, the tests fail until these
copies are refreshed.

## Licence

Both crates are MIT-licensed, `Copyright 2018-2026 the Deno authors`, as
the header of each file states (<https://github.com/denoland/deno>).

> Permission is hereby granted, free of charge, to any person obtaining a
> copy of this software and associated documentation files (the
> "Software"), to deal in the Software without restriction, including
> without limitation the rights to use, copy, modify, merge, publish,
> distribute, sublicense, and/or sell copies of the Software, and to permit
> persons to whom the Software is furnished to do so, subject to the
> following conditions:
>
> The above copyright notice and this permission notice shall be included
> in all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
> OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
> MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN
> NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
> DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
> OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
> USE OR OTHER DEALINGS IN THE SOFTWARE.
