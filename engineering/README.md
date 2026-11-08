# NextRS engineering reference

Technical reference for contributors and anyone evaluating NextRS in depth.
For an introduction, see the [project README](../README.md).

| Reference | Contents |
|---|---|
| [Architecture](architecture.md) | Request lifecycle, the V8 isolate pool, cache, networking, image optimization, repository structure, engine experiments |
| [Compatibility](compatibility.md) | Tested Next.js versions and fixtures, the full capability matrix, known gaps, unsupported features |
| [Configuration](configuration.md) | Every `nextrs` command-line option: runtime limits, network policy, cache, images, reverse proxies, metrics |
| [Security](security.md) | The full security notice: isolation limits, network policy, Server Actions, trusted proxies, unsupported-feature bypass |
| [Testing](testing.md) | Rust checks, comparison suites against `next start`, negative controls, CI |
| [Benchmarks](benchmarks.md) | Preliminary results against `next start`, method, environment, limitations, raw data, drivers |
| [Roadmap](roadmap.md) | Phase-by-phase history and candidate milestones |

The command-line help (`nextrs --help`, `nextrs start --help`) is the
authoritative list of options for the binary you built.
