# Build stage
FROM rust:1.88-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests
RUN cargo build --release

# Final stage
FROM scratch
COPY --from=builder /app/target/release/fhir-ig-testgen /fhir-ig-testgen
ENTRYPOINT ["/fhir-ig-testgen"]