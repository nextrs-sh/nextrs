# Release checksum records

These files are the checksums that `.github/workflows/npm-publish.yml` checks
before it publishes anything. Each is in `sha256sum` format.

They sit in a commit of their own. The archive records the commit it was
packaged at, so it cannot also contain its own checksum. The tag goes on
that commit's child, which changes only this directory.

## v0.1.0-rc.1

| File | What | Value |
|---|---|---|
| `nextrs-v0.1.0-rc.1-linux-x86_64.binary.sha256` | the gated binary | `44f04e586c7dc9eb0b8f69dfd992143f725472aa5f3619df5a4039d77d2c6956` |
| `nextrs-v0.1.0-rc.1-linux-x86_64.tar.gz.sha256` | the release archive | `60c353c198ad57569248710bceeb6c1bc8822674c9f19833cad5d760f7c77010` |
| `nextrs-cli-0.1.0-rc.1.tgz.sha256` | `@nextrs/cli` | `5cc83a0ba2f296a0f4a21253dfc6ee120c79cff3dbc1e50683b263ad89b986a4` |

**The binary.** It was built by release.yml run 44646067537 on `711d77f`.

**The archive.** Release.yml run 44672701580 (candidate mode) packaged that
binary at `3ae8e54` with `--built-from 711d77f`. Every job of that run passed
on it:

- the runtime-file audit;
- the compatibility suites at 1 and 4 isolates;
- clean installs on Ubuntu 22.04, Ubuntu 24.04 and Debian 12;
- the npm package job, and npm and pnpm installs on the same three images;
- refusal on unsupported platforms;
- the Docker example.

**Packaging is reproducible only with the same GNU tar.** tar 1.35 leaves
the device-number fields of each header empty; the runner's tar writes
zeros. The archive published is therefore the CI artifact itself. The npm
tarball is reproducible anywhere from that archive.
