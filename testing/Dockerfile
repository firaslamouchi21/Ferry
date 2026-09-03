FROM rust:1.97-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY bindings bindings
RUN cargo build --release --locked -p ferry-daemon -p ferry-cli

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
        dbus dbus-x11 gnome-keyring libsecret-1-0 ca-certificates procps \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/ferry-daemon /usr/local/bin/ferry-daemon
COPY --from=build /src/target/release/ferry /usr/local/bin/ferry
COPY testing/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh
ENV XDG_DATA_HOME=/data
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["ferry-daemon"]
