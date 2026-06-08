# ============================================
# Stage 1: Build stage (static compilation)
# ============================================
FROM rust:1.75-alpine3.19 AS builder
WORKDIR /app

# Install build dependencies for static compilation
RUN apk add --no-cache \
    musl-dev \
    perl \
    make \
    gcc \
    g++ \
    openssl-dev \
    openssl-libs-static

# Set environment for static compilation
ENV RUSTFLAGS="-C target-feature=+crt-static"
ENV OPENSSL_STATIC=1
ENV OPENSSL_DIR=/usr

# Create blank project to cache dependencies
RUN cargo init --name battery-monitor --bin .

# Copy Cargo files first for better caching
COPY Cargo.toml Cargo.lock ./

# Build only dependencies first (cached layer)
RUN cargo build --release 2>/dev/null || true

# Copy source code and config
COPY src ./src
COPY model_config.json ./

# Build with static linking
RUN cargo build --release && \
    strip /app/target/release/battery-monitor

# Verify static linking
RUN ldd /app/target/release/battery-monitor 2>&1 || \
    echo "Binary is statically linked"

# ============================================
# Stage 2: Runtime (minimal image with static binary)
# ============================================
FROM alpine:3.19
WORKDIR /app

# Install minimal runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    tzdata \
    curl

# Set timezone
ENV TZ=Asia/Shanghai

# Copy static binary from build stage
COPY --from=builder /app/target/release/battery-monitor /app/battery-monitor
COPY --from=builder /app/model_config.json /app/model_config.json

# Runtime environment variables
ENV RUST_LOG=info,battery_monitor=info
ENV RUST_BACKTRACE=1
ENV MODEL_CONFIG_PATH=/app/model_config.json
ENV OTEL_SERVICE_NAME=battery-monitor

# Create non-root user for security
RUN addgroup -S appgroup && adduser -S appuser -G appgroup
USER appuser

# Expose ports
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=3s --retries=3 \
    CMD curl -f http://localhost:8080/api/health || exit 1

# Start the application
CMD ["/app/battery-monitor"]
