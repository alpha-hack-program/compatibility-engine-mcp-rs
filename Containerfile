# syntax=docker/dockerfile:1

# Build arguments
ARG BASE_IMAGE=registry.access.redhat.com/ubi9/ubi-minimal
ARG BASE_TAG=9.6
ARG VERSION=2.0.2
ARG MAINTAINER="Alpha Hack Group <alpha@github.com>"
ARG DESCRIPTION="Compatibility Engine MCP Server - Model Context Protocol server to check benefits"
ARG APP_NAME=compatibility-engine-mcp-rs
ARG PORT=8000
ARG SOURCE=https://github.com/alpha-hack-program/compatibility-engine-mcp-rs.git

# Multi-stage build
# Stage 1: Build stage with Rust toolchain
FROM registry.access.redhat.com/ubi9/ubi:${BASE_TAG} AS builder

# Install Rust and build dependencies
RUN dnf update -y && \
    dnf install -y \
        gcc \
        gcc-c++ \
        make \
        openssl-devel \
        pkg-config && \
    dnf clean all && \
    rm -rf /var/cache/dnf

# Install Rust
ENV RUSTUP_HOME=/opt/rust
ENV CARGO_HOME=/opt/rust
ENV PATH=/opt/rust/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --no-modify-path

# Set working directory
WORKDIR /build

# Copy Cargo TOML
COPY Cargo.* .

# Fetch dependencies
RUN cargo fetch

# Copy the source code
COPY src/ ./src

# Build the application
RUN cargo build --release --bin mcp_server

# Stage 2: Runtime stage with minimal UBI
FROM ${BASE_IMAGE}:${BASE_TAG}

# Build arguments for labels
ARG VERSION
ARG BUILD_DATE
ARG VCS_REF
ARG MAINTAINER
ARG DESCRIPTION
ARG APP_NAME
ARG PORT
ARG SOURCE

# Add labels (OCI standard)
LABEL org.opencontainers.image.title="${APP_NAME}" \
      org.opencontainers.image.description="${DESCRIPTION}" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.created="${BUILD_DATE}" \
      org.opencontainers.image.revision="${VCS_REF}" \
      org.opencontainers.image.source="${SOURCE}" \
      org.opencontainers.image.authors="${MAINTAINER}" \
      org.opencontainers.image.vendor="Alpha Hack Group" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.url="${SOURCE}" \
      org.opencontainers.image.documentation="${SOURCE}" \
      io.k8s.description="${DESCRIPTION}" \
      io.k8s.display-name="${APP_NAME}" \
      io.openshift.tags="mcp,compatibility-engine,rust,server" \
      maintainer="${MAINTAINER}"

# Install runtime dependencies
RUN microdnf update -y && \
    microdnf install -y \
        ca-certificates \
        openssl && \
    microdnf clean all && \
    rm -rf /var/cache/yum

# Create non-root user for security
RUN useradd -r -u 1001 -g 0 -s /sbin/nologin \
    -c "Compatibility Engine MCP Server user" mcpserver

# Set working directory
WORKDIR /app

# Copy the binary from builder stage
COPY --from=builder /build/target/release/mcp_server /app/mcp_server

# Set permissions
RUN chown -R 1001:0 /app && \
    chmod -R g=u /app && \
    chmod +x /app/mcp_server

# Switch to non-root user
USER 1001

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=30s --retries=3 \
    CMD curl -f http://localhost:${PORT}/health || exit 1

# Expose port
EXPOSE ${PORT}

# Environment variables
ENV RUST_LOG=info
ENV RUST_BACKTRACE=1
ENV PORT=${PORT}

# Run the application
CMD ["/app/mcp_server"]