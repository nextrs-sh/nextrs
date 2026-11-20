# syntax=docker/dockerfile:1
#
# NextRS production image for a Next.js application.
#
# Build stage: Node.js installs the application's dependencies, including
# @nextrs/cli (a devDependency), and runs the official `next build`.
# Runtime stage: a plain Debian image with the native NextRS binary, the
# standalone build output and the static assets. It has no Node.js, no npm,
# no Rust and no source code, and nothing but NextRS serves requests.
#
# Requirements: next.config sets `output: "standalone"`, and package.json has
# "@nextrs/cli" in devDependencies (with package-lock.json committed).
# Copy this file and .dockerignore next to package.json.
#
#   docker build -t my-app .
#   docker run --rm -p 3000:3000 my-app
#
# The runtime image needs glibc 2.35 or later, libgcc_s and libm (all in
# debian:12-slim and ubuntu:22.04 or later). It needs no CA certificates
# package: NextRS has the Mozilla root store built in.

ARG NODE_IMAGE=node:22-bookworm-slim
ARG RUNTIME_IMAGE=debian:12-slim

FROM ${NODE_IMAGE} AS build
WORKDIR /app
ENV NEXT_TELEMETRY_DISABLED=1
COPY package.json package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY . .
RUN npx next build
# Check the installed binary against the checksum recorded in its package,
# run it once, and stage it with its license files for the runtime image.
RUN set -eu; pkg=node_modules/@nextrs/cli; \
    echo "$(node -p "require('./$pkg/BUILD-INFO.json').binary_sha256")  $pkg/native/nextrs" | sha256sum -c -; \
    ./node_modules/.bin/nextrs --version; \
    mkdir -p /out/doc; \
    install -m 0755 "$pkg/native/nextrs" /out/nextrs; \
    cp "$pkg/BUILD-INFO.json" "$pkg/LICENSE-MIT" "$pkg/LICENSE-APACHE" "$pkg/THIRD-PARTY.md" "$pkg/THIRD-PARTY-LICENSES.txt" /out/doc/
# The standalone output holds the server bundle and the node_modules it
# imports at run time (NextRS reads them; Node.js does not run them). Static
# assets and public/ are copied in, as for `node server.js`.
RUN set -eu; \
    test -f .next/standalone/server.js || { echo 'next.config must set output: "standalone"' >&2; exit 1; }; \
    mkdir -p .next/standalone/.next; \
    cp -r .next/static .next/standalone/.next/static; \
    if [ -d public ]; then cp -r public .next/standalone/public; fi

FROM ${RUNTIME_IMAGE}
RUN useradd --system --uid 10001 --user-group --home-dir /nonexistent --shell /usr/sbin/nologin nextrs \
 && install -d -o nextrs -g nextrs -m 0700 /var/cache/nextrs
COPY --from=build /out/nextrs /usr/local/bin/nextrs
COPY --from=build /out/doc/ /usr/share/doc/nextrs/
COPY --from=build /app/.next/standalone /app
USER nextrs
EXPOSE 3000
# SIGTERM drains in-flight requests for up to --shutdown-grace (10 s).
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/bin/nextrs"]
# Adjust for your application: --js-env NAME for each environment variable
# its JavaScript reads, --net-allow for each outbound fetch destination, and
# --trusted-proxy CIDR instead of --strip-forwarded-headers behind a proxy.
# The operations listener (/readyz, /healthz, /metrics) is on port 9100;
# do not publish it outside a private network.
CMD ["start", "/app", "--host", "0.0.0.0", "--port", "3000", "--js-runtime", "embedded-v8", \
     "--strip-forwarded-headers", \
     "--cache-dir", "/var/cache/nextrs/data", "--image-cache-dir", "/var/cache/nextrs/images", \
     "--metrics-addr", "0.0.0.0:9100", "--log-format", "json"]
