# re4-local

RE-4 end-to-end and process-memory readings. Fedora 45 host, bare metal,
inside the VS Code Flatpak sandbox, **no container limit**. Release
binary of `feat/re-4-resource-admission` at `14f9404` (the four
end-to-end runs; `process-memory.txt` was read at `550c5ff`, with the
same serving code), serving
`fixtures/next-app` (Next.js fixture build).

- `off.txt`, `admission-normal.txt`, `admission-critical.txt`,
  `budgets-young.txt`: `e2e-run.sh <label> <port> <flags>`. Each starts
  the server, checks readiness and health, gets prerendered, dynamic,
  route-handler and static paths once, fires 24 concurrent dynamic
  requests (status, `x-nextrs-error` and `Retry-After` counts), checks a
  static file and `/healthz` again, and keeps the non-zero RE-4 metrics.
  `/data-fetch` answers 500 in every run, the controls-off run included:
  the route needs `NEXTRS_UPSTREAM`, which these runs do not set.
- Memory admission read the sandbox's own cgroup (the VS Code Flatpak
  scope, about 1.85 GiB in use by the editor and everything it runs),
  reached through `/run/host/root/sys/fs/cgroup`. With a 1000 MiB budget
  that is the critical state: a live example of a cgroup shared with
  other processes, which admission counts and cannot tell apart.
- `process-memory.txt`: idle RSS / anonymous / file-backed memory with no
  executor, 1 and 2 isolates (for RE-4-FAULT-CONTAINMENT-ADR).
