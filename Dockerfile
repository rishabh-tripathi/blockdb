# Multi-stage Docker build for BlockDB
# Stage 1: Build environment
FROM rust:1.75-slim as builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /app

# Copy dependency files first for better caching
COPY Cargo.toml Cargo.lock ./

# Create a dummy main.rs to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies (cached layer)
RUN cargo build --release && rm -rf src

# Copy source code
COPY src ./src

# Build the actual application
RUN cargo build --release

# Stage 2: Runtime environment
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create blockdb user
RUN groupadd -r blockdb && useradd -r -g blockdb blockdb

# Create directories
RUN mkdir -p /var/lib/blockdb /var/log/blockdb /etc/blockdb && \
    chown -R blockdb:blockdb /var/lib/blockdb /var/log/blockdb /etc/blockdb

# Copy binaries from builder stage
COPY --from=builder /app/target/release/blockdb-server /usr/local/bin/
COPY --from=builder /app/target/release/blockdb-cli /usr/local/bin/

# Copy configuration
COPY docker/blockdb.toml /etc/blockdb/blockdb.toml
COPY docker/entrypoint.sh /usr/local/bin/entrypoint.sh

# Make scripts executable
RUN chmod +x /usr/local/bin/entrypoint.sh

# Set user
USER blockdb

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Expose ports
EXPOSE 8080 9090

# Set work directory
WORKDIR /var/lib/blockdb

# Environment variables
ENV BLOCKDB_CONFIG=/etc/blockdb/blockdb.toml
ENV BLOCKDB_DATA_DIR=/var/lib/blockdb
ENV BLOCKDB_LOG_LEVEL=info

# Use entrypoint script
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["server"]