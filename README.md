# Compatibility Engine MCP Server

> **Example Model Context Protocol (MCP) Server providing five calculation and compatibility functions**

[![CI Pipeline](https://github.com/alpha-hack-program/eligibility-engine-mcp-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/alpha-hack-program/eligibility-engine-mcp-rs/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)

An example Model Context Protocol (MCP) server developed in Rust that provides five strongly-typed calculation and compatibility functions. This project demonstrates how to build MCP servers with explicit computational logic.

## ⚠️ **DISCLAIMER**

This server provides five calculation functions that demonstrate various computational patterns commonly used in business applications. All calculations are explicit and transparent.

**This is a demonstration/example project only.** The calculations and logic implemented here are for educational and demonstration purposes. This software:

- **Should NOT be used for actual financial or legal decisions**
- **Does NOT represent real business calculations**
- **Is NOT affiliated with any official entity**
- **Serves as a technical example of MCP server implementation**

For real financial or legal calculations, please consult appropriate professional services.

## 🎯 Features

- **5 Calculation Functions**: calc_penalty, calc_tax, check_voting, distribute_waterfall, check_housing_grant
- **Explicit Logic**: No external dependencies - all logic is transparent and verifiable
- **Multiple Transport Protocols**: Examples of STDIO, SSE, and HTTP streamable implementations
- **Robust Input Validation**: Demonstrates JSON schema validation with detailed error handling
- **Production-Ready Containerization**: Example Docker/Podman setup for deployment
- **Claude Desktop Integration**: Example DXT packaging for MCP integration
- **Professional Version Management**: Automated version sync with cargo-release
- **CI/CD Pipeline**: Comprehensive GitHub Actions workflow
- **Professional Repository Structure**: Organized scripts and clean project layout

## 📚 Quick Reference

| Task | Command | Description |
|------|---------|-------------|
| **🏗️ Build** | `make build-all` | Build all servers |
| **🧪 Test** | `make test` | Run all tests |
| **🚀 Release** | `make release-patch` | Create new patch release |
| **📦 Package** | `make pack` | Create Claude Desktop package |
| **🐳 Container** | `scripts/image.sh build` | Build container image |
| **🔄 Sync** | `make sync-version` | Sync versions manually |
| **ℹ️ Help** | `make help` | Show all commands |

## 📋 Available Functions

| Function | Description | Example |
|----------|-------------|----------|
| **calc_penalty** | Calculate penalty with cap and interest | 12 days late × $100/day = $1,050 with interest |
| **calc_tax** | Progressive tax with surcharge | $40,000 income = $7,140 tax with surcharge |
| **check_voting** | Check voting proposal eligibility | 70 out of 100 voters, 55 yes votes = passes |
| **distribute_waterfall** | Distribute cash in waterfall structure | $15M → Senior: $8M, Junior: $7M, Equity: $0 |
| **check_housing_grant** | Check housing grant eligibility | AMI $50K, size 5, income $32K = eligible |

> **Note**: These functions demonstrate common calculation patterns.

## 🚀 Quick Start

### Prerequisites

- Rust 1.70+ ([Install Rust](https://rustup.rs/))
- Cargo (included with Rust)
- `jq` for JSON processing ([Install jq](https://jqlang.github.io/jq/download/))
- `cargo-release` for version management: `cargo install cargo-release`

### Installation

```bash
# Clone the repository
git clone https://github.com/alpha-hack-program/compatibility-engine-mcp-rs.git
cd compatibility-engine-mcp-rs

# Build all servers
make build-all

# Or build individually
make build-sse      # SSE Server
make build-mcp      # MCP HTTP Server
make build-stdio    # STDIO Server for Claude
```

### Running

```bash
# SSE Server (recommended for development)
make test-sse

# MCP HTTP Server
make test-mcp

# Or directly
RUST_LOG=debug ./target/release/sse_server
```

## 🔧 Configuration

### Environment Variables

```bash
# Server configuration
RUST_LOG=info           # Logging level (debug, info, warn, error)

# Or use BIND_ADDRESS directly
BIND_ADDRESS=127.0.0.1:8000
```

### Example Usage

#### Calculate Penalty with Interest

```json
{
  "days_late": 12,
  "rate_per_day": 100,
  "cap": 1000,
  "interest_rate": 0.05
}
```

**Response:** `1050.0` (penalty capped at $1000 + 5% interest = $1050)

#### Calculate Progressive Tax

```json
{
  "income": 40000,
  "thresholds": [10000],
  "rates": [0.10, 0.20],
  "surcharge_threshold": 5000,
  "surcharge_rate": 0.02
}
```

**Response:** `7140.0` (progressive tax $7000 + 2% surcharge = $7140)

> **Important**: These are example calculations for demonstration purposes only.

## 🐳 Containerization

### Build and Run

This requires `podman` or `docker`. Configuration is managed through `.env` file.

```bash
# Build container image
scripts/image.sh build

# Run locally
scripts/image.sh run

# Run from remote registry
scripts/image.sh push
scripts/image.sh run-remote

# Show container information
scripts/image.sh info
```

### Environment Variables for Containers

```bash
# Production configuration
podman run -p 8001:8001 \
  -e BIND_ADDRESS=0.0.0.0:8001 \
  -e RUST_LOG=info \
  quay.io/atarazana/compatibility-engine-mcp-server:latest
```

## 📦 Claude Desktop Integration

### Packaging

```bash
# Create DXT package for Claude Desktop
$ make pack
cargo build --release --bin stdio_server
   Compiling compatibility-engine-mcp-server v1.0.8 (/Users/.../compatibility-engine-mcp-rs)
    Finished `release` profile [optimized] target(s) in 18.23s
Packing MCP server for Claude Desktop...
chmod +x ./target/release/stdio_server
zip -rX compatibility-engine-mcp-server.dxt -j dxt/manifest.json ./target/release/stdio_server
updating: manifest.json (deflated 49%)
updating: stdio_server (deflated 63%)
```

### Example Claude Configuration

Drag and drop the `DXT` file into the `Settings->Extensions` dropping area.

> **Note**: This demonstrates MCP integration patterns and is not intended for production use with real data.




## 🧪 Testing

```bash
# Run all tests
make test
```



## 🛠️ Development

### Available Commands

#### 🏗️ Build Commands
```bash
make build-all              # Build all servers
make build-mcp              # Build MCP server (streamable-http)
make build-sse              # Build SSE server
make build-stdio            # Build stdio server
make pack                   # Pack MCP server for Claude Desktop
```

#### 🚀 Release Commands (cargo-release)
```bash
make release-patch          # Create patch release (1.0.6 → 1.0.7)
make release-minor          # Create minor release (1.0.6 → 1.1.0)
make release-major          # Create major release (1.0.6 → 2.0.0)
make release-dry-run        # Show what release-patch would do
make sync-version           # Manually sync version to all files
```

#### 🧪 Test Commands
```bash
make test                   # Run all tests
make test-sse               # Test SSE server locally
make test-mcp               # Test MCP server locally
```

#### 🔧 Development Commands
```bash
make clean                  # Clean build artifacts
make help                   # Show all available commands
```

### Project Structure

```
├── src/                                    # Source code
│   ├── common/
│   │   ├── compatibility_engine.rs       # MCP logic and calculation functions
│   │   └── mod.rs
│   ├── sse_server.rs                      # SSE Server
│   ├── mcp_server.rs                      # MCP HTTP Server
│   └── stdio_server.rs                    # STDIO Server
├── scripts/                               # Utility scripts
│   ├── sync-manifest-version.sh           # Version sync for cargo-release
│   └── image.sh                          # Container management script
├── dxt/
│   └── manifest.json                      # Claude Desktop manifest
├── .github/workflows/                     # CI/CD pipelines
│   └── ci.yml                            # GitHub Actions workflow
├── docs/                                  # Documentation
├── .env                                   # Environment variables
├── Containerfile                          # Container definition
├── Cargo.toml                            # Rust package manifest
└── Makefile                              # Build commands
```

### Debug and Monitoring

First run the SSE server (or the Streamable HTTP version with `make test-mcp`):

```bash
$ make test-sse
cargo build --release --bin sse_server
   Compiling compatibility-engine-mcp-server v1.0.6 (/Users/cvicensa/Projects/rust/claude/compatibility-engine-mcp-rs)
    Finished `release` profile [optimized] target(s) in 18.26s
🧪 Testing SSE server...

RUST_LOG=debug ./target/release/sse_server
2025-09-22T16:53:01.931985Z  INFO sse_server: Starting sse Compatibility Engine MCP server on 127.0.0.1:8000
```

Second, run MCP inspector:

> **NOTE:** NodeJS 19+ has to be installed

```bash
$ make inspector
npx @modelcontextprotocol/inspector
Starting MCP inspector...
⚙️ Proxy server listening on 127.0.0.1:6277
🔑 Session token: 6f0fdc22e2a9775a95d60c976b37b873bffec1816002fc702ca8ec7186a7c338
Use this token to authenticate requests or set DANGEROUSLY_OMIT_AUTH=true to disable auth

🔗 Open inspector with token pre-filled:
   http://localhost:6274/?MCP_PROXY_AUTH_TOKEN=6f0fdc22e2a9775a95d60c976b37b873bffec1816002fc702ca8ec7186a7c338

🔍 MCP Inspector is up and running at http://127.0.0.1:6274 🚀
```

Open a browser and point to the URL with the token included.

Troubleshooting:

MCP error -32602: failed to deserialize parameters: missing field `is_single_parent`

Just click on the checkbox `is_single_parent` and try again.

Additional targets:

```bash
# Debug proxy
make proxy                  # Start mitmproxy on port 8888

# Supergateway for SSE
make sgw-sse               # STDIO -> SSE wrapping

# Supergateway for MCP
make sgw-mcp               # STDIO -> MCP HTTP wrapping
```

## 📚 API Reference

### Main Endpoint

**POST** `/message` - Example endpoint for rule evaluation

### Function Parameters

#### calc_penalty
| Field | Type | Description |
|-------|------|-------------|
| `days_late` | number | Number of days late |
| `rate_per_day` | number | Rate per day |
| `cap` | number | Maximum penalty cap |
| `interest_rate` | number | Interest rate (decimal) |

#### calc_tax
| Field | Type | Description |
|-------|------|-------------|
| `income` | number | Total income |
| `thresholds` | array | Tax bracket thresholds |
| `rates` | array | Tax rates for each bracket |
| `surcharge_threshold` | number | Surcharge threshold |
| `surcharge_rate` | number | Surcharge rate (decimal) |

#### check_voting
| Field | Type | Description |
|-------|------|-------------|
| `eligible_voters` | integer | Total eligible voters |
| `turnout` | integer | Actual turnout |
| `yes_votes` | integer | Number of yes votes |
| `proposal_type` | string | "general" or "amendment" |

#### distribute_waterfall
| Field | Type | Description |
|-------|------|-------------|
| `cash_available` | number | Total cash to distribute |
| `senior_debt` | number | Senior debt amount |
| `junior_debt` | number | Junior debt amount |

#### check_housing_grant
| Field | Type | Description |
|-------|------|-------------|
| `ami` | number | Area Median Income |
| `household_size` | integer | Household size |
| `income` | number | Household income |
| `has_other_subsidy` | boolean | Whether household has another subsidy |

## 🔒 Security

- **Input validation**: Strict JSON schemas
- **Non-root user**: Containers run as user `1001`
- **Security audit**: `cargo audit` in CI/CD
- **Minimal image**: Based on UBI 9 minimal

## 🤝 Contributing

### Development Workflow

1. **Fork the project**
2. **Create feature branch**: `git checkout -b feature/new-feature`
3. **Make changes and test**: `make test`
4. **Commit changes**: `git commit -am 'Add new feature'`
5. **Push to branch**: `git push origin feature/new-feature`
6. **Create Pull Request**

### Professional Release Process

1. **Development**: Make changes, test with `make test`
2. **Version Bump**: Use `make release-patch/minor/major`
3. **Build**: Use `make pack` for Claude Desktop integration
4. **Container**: Use `scripts/image.sh build` for containerization

### Guidelines

- **Code Quality**: Follow `cargo fmt` and pass `cargo clippy`
- **Testing**: Add tests for new functionality
- **Version Management**: Let cargo-release handle versioning
- **CI/CD**: Ensure all GitHub Actions pass
- **Documentation**: Update README.md as needed
- **Professional Structure**: Keep scripts in `scripts/` directory

## ⚙️ Version Management

This project uses **cargo-release** for professional version management with automatic synchronization across all configuration files.

### 🔄 Version Sync System

- **Single Source of Truth**: `Cargo.toml` version controls everything
- **Automatic Sync**: Updates `dxt/manifest.json` and `.env` automatically
- **Git Integration**: Creates commits and tags automatically

### 📦 Release Workflow

```bash
# 1. Make your changes and commit them
git add -A && git commit -m "feat: your changes"

# 2. Create a release (choose appropriate version bump)
make release-patch     # Bug fixes: 1.0.6 → 1.0.7
make release-minor     # New features: 1.0.6 → 1.1.0  
make release-major     # Breaking changes: 1.0.6 → 2.0.0

# 3. Build and package
make pack
scripts/image.sh build
scripts/image.sh push

# 4. Push to repository
git push && git push --tags
```

### 🔍 Preview Changes

```bash
# See what would happen without making changes
make release-dry-run
```

### 🛠️ Manual Version Sync (Development)

```bash
# Sync version from Cargo.toml to other files manually
make sync-version
```

## 📄 License

This project is licensed under the MIT License - see [LICENSE](LICENSE) for details.

## 🚀 Production Deployment

### Environment Configuration

The project uses `.env` for environment management:

```bash
# Version (automatically managed by cargo-release)
VERSION=1.0.6

# Container Configuration  
APP_NAME="compatibility-engine-mcp-rs"
BASE_IMAGE="registry.access.redhat.com/ubi9/ubi-minimal"
PORT=8001

# Registry Configuration
REGISTRY=quay.io/atarazana
```

### CI/CD Pipeline

The project includes a comprehensive GitHub Actions workflow:
- ✅ **Automated Testing**: Unit tests and integration tests
- ✅ **Version Sync Validation**: Tests cargo-release functionality  
- ✅ **Container Building**: Tests containerization process
- ✅ **Artifact Management**: Builds and uploads release artifacts
- ✅ **Cross-platform Support**: Tests on Ubuntu with multiple container runtimes

## 🙋 Support

- **Issues**: [GitHub Issues](https://github.com/alpha-hack-program/compatibility-engine-mcp-rs/issues)
- **Documentation**: [Project Wiki](https://github.com/alpha-hack-program/compatibility-engine-mcp-rs/wiki)
- **CI/CD**: Automated testing and deployment via GitHub Actions

## 🏷️ Tags

`mcp` `model-context-protocol` `rust` `compatibility-engine` `calculations` `explicit-logic` `claude` `computation-engine` `cargo-release` `professional-rust` `containerization` `ci-cd`

---

**Developed with ❤️ by [Alpha Hack Group](https://github.com/alpha-hack-program)**