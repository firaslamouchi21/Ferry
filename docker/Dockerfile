FROM rust:1.97.1-bookworm AS rust-build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY bindings bindings
RUN cargo build --release --locked -p ferry-daemon -p ferry-cli

FROM node:22-bookworm-slim AS ui-build
RUN corepack enable
WORKDIR /src
COPY package.json pnpm-workspace.yaml pnpm-lock.yaml ./
COPY ui/package.json ui/package.json
COPY desktop/package.json desktop/package.json
COPY ext/vscode/package.json ext/vscode/package.json
RUN pnpm install --frozen-lockfile --filter @ferry/ui...
COPY bindings bindings
COPY ui ui
RUN pnpm --filter @ferry/ui build

FROM node:22-bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=rust-build /src/target/release/ferry-daemon /usr/local/bin/ferry-daemon
COPY --from=rust-build /src/target/release/ferry /usr/local/bin/ferry
COPY --from=ui-build /src/ui/dist /app/ui-dist
COPY ui/dev-bridge/bridge.js ui/dev-bridge/serve.mjs /app/dev-bridge/
RUN npm install --no-save --prefix /app ws@8.21.3
COPY docker/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

ENV XDG_DATA_HOME=/data
ENV FERRY_UI_DIST=/app/ui-dist
ENV PORT=5170
VOLUME ["/data"]
EXPOSE 5170 47821/udp 47821/tcp

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
