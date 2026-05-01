# Use the pre-built ESP Rust image
FROM espressif/idf-rust:all_latest

# Image runs as user 'esp' by default
# Install ARM targets (RP2040 + RP2350) and elf2uf2-rs in the nightly toolchain.
RUN rustup target add --toolchain nightly thumbv6m-none-eabi thumbv8m.main-none-eabihf \
 && cargo +nightly install elf2uf2-rs

# Make the home directory, cargo, and rustup dirs accessible to any user.
# This allows running the container with `--user $(id -u):$(id -g)` in CI environments
# so that all generated files are owned by the host runner by default.
USER root
RUN chmod 755 /home/esp /home/esp/.cargo /home/esp/.cargo/bin /home/esp/.rustup
USER esp

WORKDIR /app

# The workspace should be mounted to /app at runtime.
# Example usage:
# docker build -t cw-adapter-builder .
# docker run --rm -v $(pwd):/app cw-adapter-builder ./scripts/build-web.sh
