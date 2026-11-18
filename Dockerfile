# Stage 1: Build React + TypeScript Frontend Console
FROM node:20-alpine AS frontend-builder
WORKDIR /app/frontend

COPY frontend/package*.json ./
RUN npm install --no-audit --no-fund

COPY frontend/ ./
RUN npm run build

# Stage 2: Build Release Rust Backend Engine (pinned to Bookworm glibc matching runtime)
FROM rust:1-slim-bookworm AS backend-builder
WORKDIR /app/backend

# Cache dependencies before copying source
COPY backend/Cargo.toml ./
RUN mkdir -p src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src

COPY backend/src ./src
COPY backend/tests ./tests
RUN touch src/main.rs && cargo build --release && strip target/release/vulcan-engine

# Stage 3: Ultra-Minimal Production Runtime (< 40 MB)
FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN useradd --system --create-home --uid 10001 vulcan && \
    mkdir -p /app/data && \
    chown -R vulcan:vulcan /app

COPY --from=backend-builder /app/backend/target/release/vulcan-engine /app/vulcan-engine
COPY --from=frontend-builder /app/frontend/dist /app/frontend/dist

ENV PORT=8080 \
    VULCAN_STATIC_DIR=/app/frontend/dist \
    VULCAN_SNAPSHOT_PATH=/app/data/vulcan_snapshot.json

USER vulcan
EXPOSE 8080

CMD ["/app/vulcan-engine"]
