# Build stage
FROM rust:1.88-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests
RUN cargo build --release

# CA certificates stage
FROM alpine:latest AS certs
RUN apk add --no-cache ca-certificates

# Final stage
FROM scratch
COPY --from=certs /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder /app/target/release/fhir-ig-testgen /fhir-ig-testgen
ENTRYPOINT ["/fhir-ig-testgen"]