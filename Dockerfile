# Builds the one static binary and ships nothing else: no shell, no package
# manager, no libc. The Rust version here must match rust-toolchain.toml (CI
# checks that); it is not copied into the build stage because the file also
# asks for rustfmt and clippy, which a release build does not need.

FROM rust:1.100.0-alpine3.24@sha256:de1e0058543a81ed7b046629167db587c67a94bdf37cce14eb27f0ec83baaffb AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
# The page's scripts and styles and the vendored libraries are embedded in the
# binary (src/assets.rs).
COPY web ./web
COPY vendor ./vendor
# Alpine's Rust target is musl, which links statically by default.
RUN cargo build --release --locked

FROM scratch
LABEL org.opencontainers.image.source="https://github.com/FilipeFantini/enlightenviz" \
      org.opencontainers.image.licenses="Apache-2.0"
COPY --from=build /src/target/release/enlightenviz /enlightenviz
# A numeric user: the image has no /etc/passwd to look a name up in.
USER 65532:65532
ENV ENLIGHTENVIZ_BOARDS=/boards \
    ENLIGHTENVIZ_LISTEN=0.0.0.0:8080
EXPOSE 8080
ENTRYPOINT ["/enlightenviz"]
CMD ["serve"]
