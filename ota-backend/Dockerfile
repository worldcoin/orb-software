# A very small, self-contained image for the `orb-ota-backend` binary.

# ---------- Build stage ----------
FROM rust:1.76 as builder

# Create app directory inside the container and copy crate sources
WORKDIR /app
COPY . .

# Build the release binary
RUN cargo build --release --bin orb-ota-backend

# ---------- Runtime stage ----------
FROM debian:bookworm-slim as runtime

# Install certificates so TLS connections made by the service work out of the box
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy the statically-linked (or mostly-static) binary from the builder
COPY --from=builder /app/target/release/orb-ota-backend /usr/local/bin/orb-ota-backend

# Expose the port the service listens on
EXPOSE 3000

# The service requires a `DATABASE_URL` environment variable at runtime. Nothing
# is set here so that it can be provided when the container is run.

CMD ["orb-ota-backend"]
