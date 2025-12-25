# Stage 1: Build frontend
FROM node:22-alpine AS frontend-builder
WORKDIR /app
COPY package*.json ./
RUN npm ci
COPY index.html vite.config.ts tsconfig*.json ./
COPY src/ src/
COPY public/ public/
RUN npm run build

# Stage 2: Build Rust backend
FROM rust:1.92-slim-bookworm AS rust-builder
WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && mkdir -p src/pipeline && echo "fn main() {}" > src/pipeline/main.rs
RUN cargo build --release || true
RUN rm -rf src

# Build actual application
COPY src/ src/
RUN cargo build --release --bin orderflow-bubbles

# Stage 3: Runtime
FROM debian:bookworm-slim
WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

# Copy built artifacts
COPY --from=rust-builder /app/target/release/orderflow-bubbles ./
COPY --from=frontend-builder /app/dist ./dist

# Copy audio files if they exist
COPY --from=frontend-builder /app/public/*.mp3 ./dist/ 2>/dev/null || true

ENV RUST_LOG=info
ENV PORT=8080

EXPOSE 8080

CMD ["./orderflow-bubbles", "--demo"]
