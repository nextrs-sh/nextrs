# NextRS engineering reference

Technical reference for contributors and anyone evaluating NextRS in depth.
For an introduction, see the [project README](../README.md).

| Reference | Contents |
|---|---|
| [Installation](installation.md) | npm, pnpm and standalone installation, requirements, supported platforms, production process model, Docker, troubleshooting |
| [Distribution](distribution.md) | How one release binary reaches GitHub Releases and npm unchanged; the npm package layout and launcher; packaging tests |
| [Architecture](architecture.md) | Request lifecycle, the V8 isolate pool, cache, networking, image optimization, repository structure, engine experiments |
| [Compatibility](compatibility.md) | Tested Next.js versions and fixtures, the full capability matrix, known gaps, unsupported features |
| [Doctor](doctor.md) | **v0.1.0, from 0.1.0-rc.3 (not in rc.1 or rc.2):** `nextrs doctor`, read-only diagnostics of whether an application and its build suit NextRS; checks, severities, exit codes, JSON output |
| [Configuration](configuration.md) | Every `nextrs` command-line option: runtime limits, network policy, cache, images, reverse proxies, metrics |
| [Operations](operations.md) | Deployment checklist, probes, shutdown, metrics to watch, runbook |
| [Security](security.md) | The full security notice: isolation limits, network policy, Server Actions, trusted proxies, unsupported-feature bypass |
| [Testing](testing.md) | Rust checks, comparison suites against `next start`, negative controls, CI |
| [Benchmarks](benchmarks.md) | Preliminary results against `next start`, method, environment, limitations, raw data, drivers |
| [Roadmap](roadmap.md) | Phase-by-phase history and candidate milestones |

The command-line help (`nextrs --help`, `nextrs start --help`) is the
authoritative list of options for the binary you built.
