# Multi-stage build for minimal image size
FROM rust:1.93-slim as builder

WORKDIR /build

# Install dependencies
RUN apt-get update && apt-get install -y \
    protobuf-compiler \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

# Build release binary
RUN cargo build --release --bin deriva-server

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 deriva

# Copy binary from builder
COPY --from=builder /build/target/release/deriva-server /usr/local/bin/

# Create data directory
RUN mkdir -p /data && chown deriva:deriva /data

USER deriva
WORKDIR /data

# Expose gRPC port
EXPOSE 50051

# Expose metrics/admin port (for Phase 2.5)
EXPOSE 9090

CMD ["deriva-server"]
