# syntax=docker/dockerfile:1.10
#
# Velasia Airways storefront served by the PUBLISHED NextRS npm release candidate
# (@nextrs/cli@0.1.0-rc.1). Integration harness only: Velasia Airways is a read-only input.
#
# Build contexts:
#   default   this directory (integration package.json/package-lock.json, scripts)
#   velasia   the Velasia Airways checkout (named context, read-only; its own .dockerignore
#             applies, so its node_modules, .next, .git, .env files and databases never enter)
#
# Stages:
#   A velasia-build   Velasia's own build, exactly as its docker/Dockerfile.web does it:
#                     pnpm 10.23.0 (corepack), `pnpm install --frozen-lockfile`,
#                     `pnpm --filter @velasia/web build` (next build, output: 'standalone',
#                     a throwaway build-time retailing API on a temporary TalaDB file).
#   B nextrs-install  `npm ci` of this directory's lockfile: @nextrs/cli@0.1.0-rc.1 from
#                     registry.npmjs.org. Checks version, checksum, platform, linked libraries.
#                     Nothing is compiled; there is no Rust toolchain anywhere in this file.
#   C nextrs-inspect  `nextrs inspect` against Velasia's standalone output (fails the build if
#                     NextRS cannot read it), report kept in the image.
#   D runtime         debian:12-slim (glibc 2.36). The native nextrs binary + the standalone
#                     output. NO Node.js, npm, pnpm or `next` server process can run here:
#                     there is no node executable in this image.

ARG NODE_IMAGE=node:24.18.0-bookworm-slim@sha256:6f7b03f7c2c8e2e784dcf9295400527b9b1270fd37b7e9a7285cf83b6951452d
ARG RUNTIME_IMAGE=debian:12-slim@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc

# ---------------------------------------------------------------------------------------------
# Stage A: build Velasia Airways (Node.js is allowed here: building only).
FROM ${NODE_IMAGE} AS velasia-build
ENV PNPM_HOME=/pnpm \
    CI=true \
    COREPACK_ENABLE_DOWNLOAD_PROMPT=0 \
    TURBO_TELEMETRY_DISABLED=1 \
    NEXT_TELEMETRY_DISABLED=1
RUN corepack enable
WORKDIR /repo
COPY --from=velasia package.json pnpm-lock.yaml pnpm-workspace.yaml .npmrc ./
COPY --from=velasia apps/retailing-api/package.json apps/retailing-api/
COPY --from=velasia apps/web/package.json apps/web/
COPY --from=velasia packages/config/package.json packages/config/
COPY --from=velasia packages/contracts/package.json packages/contracts/
COPY --from=velasia packages/observability/package.json packages/observability/
COPY --from=velasia packages/ui/package.json packages/ui/
RUN --mount=type=cache,id=nextrs-velasia-pnpm,target=/pnpm/store \
    pnpm install --frozen-lockfile --filter "@velasia/web..." --filter "@velasia/retailing-api..."
COPY --from=velasia packages packages
COPY --from=velasia apps/retailing-api apps/retailing-api
COPY --from=velasia apps/web apps/web
# Public build settings, the same ones Velasia's own image takes (docker/Dockerfile.web).
ARG SITE_URL=http://localhost:3000
ARG VELASIA_TODAY=
ENV SITE_URL=${SITE_URL} \
    VELASIA_TODAY=${VELASIA_TODAY}
# Velasia's build (apps/web/scripts/with-retailing-api.mjs) prerenders destination pages from the
# retailing API. It starts a throwaway API itself but gives it only 30 s to become healthy, which
# a loaded Docker VM can miss. Its documented alternative: "If RETAILING_API_URL already answers
# /health, it is used as-is." So start the very same API the same way (same command, a temporary
# TalaDB file, LOG_LEVEL=warn), wait up to BUILD_API_WAIT seconds, run Velasia's unchanged build,
# then stop the API and delete its data. Nothing about the build or its output changes.
ARG BUILD_API_WAIT=180
RUN set -eu; \
    data=$(mktemp -d /tmp/velasia-build-api-XXXXXX); \
    RETAILING_API_HOST=127.0.0.1 RETAILING_API_PORT=4010 LOG_LEVEL=warn TALADB_PATH="$data/velasia.db" \
      setsid pnpm --filter @velasia/retailing-api start & api=$!; \
    i=0; until node -e "fetch('http://127.0.0.1:4010/health').then(r=>process.exit(r.ok?0:1),()=>process.exit(1))"; do \
      i=$((i+1)); [ "$i" -le $((BUILD_API_WAIT*2)) ] && kill -0 "$api" 2>/dev/null \
        || { echo "build-time retailing API did not become healthy" >&2; kill -TERM "-$api" 2>/dev/null; exit 1; }; \
      sleep 0.5; done; \
    echo "build-time retailing API healthy after ~$((i/2)) s"; \
    status=0; RETAILING_API_URL=http://127.0.0.1:4010 pnpm --filter @velasia/web build || status=$?; \
    kill -TERM "-$api" 2>/dev/null || true; wait "$api" 2>/dev/null || true; \
    rm -rf "$data"; exit "$status"
# The same post-build check as Velasia's image: the standalone server exists, no database or
# .env file leaked into it.
RUN test -f apps/web/.next/standalone/apps/web/server.js \
 && test -d apps/web/.next/standalone/apps/web/.next/static \
 && test -d apps/web/.next/standalone/apps/web/public \
 && ! find apps/web/.next/standalone -name '*.db' -o -name '.env*' | grep -q . \
 && node -p "'next ' + require('/repo/apps/web/node_modules/next/package.json').version" \
      > /repo/next-version.txt \
 && cat /repo/next-version.txt

# ---------------------------------------------------------------------------------------------
# Stage B: install the published npm release candidate (no compilation).
FROM ${NODE_IMAGE} AS nextrs-install
WORKDIR /integration
COPY package.json package-lock.json ./
# --ignore-scripts: the package declares none; this guarantees nothing runs on install.
RUN --mount=type=cache,id=nextrs-velasia-npm,target=/root/.npm \
    npm ci --no-audit --no-fund --ignore-scripts --registry=https://registry.npmjs.org/
RUN set -eu; pkg=node_modules/@nextrs/cli; \
    v=$(node -p "require('./$pkg/package.json').version"); \
    [ "$v" = "0.1.0-rc.1" ] || { echo "expected @nextrs/cli 0.1.0-rc.1, got $v" >&2; exit 1; }; \
    node -e "const l=require('./package-lock.json').packages['node_modules/@nextrs/cli']; \
      if (!l.resolved.startsWith('https://registry.npmjs.org/')) { console.error('not from the npm registry: '+l.resolved); process.exit(1) } \
      console.log('resolved', l.resolved); console.log('integrity', l.integrity)"; \
    echo "$(node -p "require('./$pkg/BUILD-INFO.json').binary_sha256")  $pkg/native/nextrs" | sha256sum -c -; \
    out=$(./node_modules/.bin/nextrs --version); echo "$out"; \
    [ "$out" = "nextrs 0.1.0-rc.1" ] || { echo "unexpected --version: $out" >&2; exit 1; }; \
    ldd "$pkg/native/nextrs"; \
    mkdir -p /out/doc; \
    install -m 0755 "$pkg/native/nextrs" /out/nextrs; \
    cp "$pkg/package.json" /out/doc/npm-package.json; \
    cp package-lock.json /out/doc/integration-package-lock.json; \
    cp "$pkg/BUILD-INFO.json" "$pkg/LICENSE-MIT" "$pkg/LICENSE-APACHE" \
       "$pkg/THIRD-PARTY.md" "$pkg/THIRD-PARTY-LICENSES.txt" /out/doc/

# ---------------------------------------------------------------------------------------------
# Stage C: NextRS must recognise Velasia's build output. Run on the runtime base, as the
# runtime will, so a glibc mismatch fails here too.
FROM ${RUNTIME_IMAGE} AS nextrs-inspect
COPY --from=nextrs-install /out/nextrs /usr/local/bin/nextrs
COPY --from=velasia-build /repo/apps/web/.next/standalone /app
RUN set -eu; mkdir -p /out; \
    /usr/local/bin/nextrs inspect /app/apps/web > /out/velasia-inspect.txt 2>&1 \
      || { cat /out/velasia-inspect.txt; echo "nextrs inspect failed" >&2; exit 1; }; \
    cat /out/velasia-inspect.txt

# ---------------------------------------------------------------------------------------------
# Stage D: runtime. Only NextRS can serve requests from this image.
FROM ${RUNTIME_IMAGE} AS runtime
LABEL org.opencontainers.image.title="nextrs-velasia-web" \
      org.opencontainers.image.description="Velasia Airways storefront served by @nextrs/cli@0.1.0-rc.1 (integration test image)" \
      dev.nextrs.npm-package="@nextrs/cli@0.1.0-rc.1" \
      dev.nextrs.binary-sha256="44f04e586c7dc9eb0b8f69dfd992143f725472aa5f3619df5a4039d77d2c6956"
RUN useradd --system --uid 10001 --user-group --home-dir /nonexistent --shell /usr/sbin/nologin nextrs \
 && install -d -o nextrs -g nextrs -m 0700 /var/cache/nextrs
COPY --from=nextrs-install /out/nextrs /usr/local/bin/nextrs
COPY --from=nextrs-install /out/doc/ /usr/share/doc/nextrs/
COPY --from=nextrs-inspect /out/velasia-inspect.txt /usr/share/doc/nextrs/velasia-inspect.txt
COPY --from=velasia-build /repo/next-version.txt /usr/share/doc/nextrs/velasia-next-version.txt
# The standalone output: server bundles and the node_modules they import (NextRS reads them in
# V8; Node.js never runs them), plus .next/static and public/ (copied by Velasia's own build).
COPY --from=velasia-build /repo/apps/web/.next/standalone /app
# Fail the image build if any Node.js executable slipped in.
RUN ! command -v node && ! command -v npm && ! command -v pnpm && echo "no Node.js in runtime image"
USER nextrs
EXPOSE 3000
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/bin/nextrs"]
# Overridden by compose.yaml (which adds the API allow-list for the internal network).
CMD ["start", "/app/apps/web", "--host", "0.0.0.0", "--port", "3000", "--js-runtime", "embedded-v8"]
