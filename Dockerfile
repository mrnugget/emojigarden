# Planner stage - Prepare recipe for dependencies
FROM lukemathwalker/cargo-chef:latest-rust-1.74.0-slim AS planner
WORKDIR /app
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Builder stage - Build the application with caching
FROM lukemathwalker/cargo-chef:latest-rust-1.74.0-slim AS builder
WORKDIR /app

# Copy and build dependencies first (better caching)
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Copy source and build application
COPY . .
RUN cargo build --release --bin emojigarden

# Runtime stage - Create minimal runtime image
FROM debian:bookworm-slim AS runtime
WORKDIR /app

# Install runtime dependencies in a single layer
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/* && \
    update-ca-certificates

# Copy only the binary
COPY --from=builder /app/target/release/emojigarden /usr/local/bin/

# Set user for better security
USER nobody
EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/emojigarden"]
