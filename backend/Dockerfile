# Stage 1: Build stage (static compilation)
FROM rust:1.75-alpine AS builder
WORKDIR /app

RUN apk add --no-cache musl-dev perl make

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY model_config.json ./

RUN cargo build --release

# Stage 2: Runtime (minimal image with static binary)
FROM alpine:3.19
WORKDIR /app

RUN apk add --no-cache ca-certificates tzdata

# Copy static binary from build stage
COPY --from=builder /app/target/release/battery-monitor /app/battery-monitor
COPY --from=builder /app/model_config.json /app/model_config.json

# Set timezone
ENV TZ=Asia/Shanghai

# Runtime environment variables
ENV RUST_LOG=info
ENV RUST_BACKTRACE=1
ENV MODEL_CONFIG_PATH=/app/model_config.json

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=3s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost:8080/api/health || exit 1

CMD ["/app/battery-monitor"]
