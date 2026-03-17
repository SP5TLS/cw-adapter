# Use the pre-built ESP Rust image
FROM espressif/idf-rust:all_1.84.0.0

# Image runs as user 'esp' by default
# Install thumbv6m target and elf2uf2-rs for RP2040 in the nightly toolchain
RUN rustup target add --toolchain nightly thumbv6m-none-eabi \
 && cargo +nightly install elf2uf2-rs

WORKDIR /app

# The workspace should be mounted to /app at runtime.
# Example usage:
# docker build -t cw-adapter-builder .
# docker run --rm -v $(pwd):/app cw-adapter-builder ./scripts/build-web.sh
