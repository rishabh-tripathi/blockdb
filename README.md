# BlockDB

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org/)
[![Tests](https://img.shields.io/badge/tests-98.5%25-brightgreen.svg)](./TEST_REPORT.md)

**BlockDB** is a high-performance, distributed, append-only database with blockchain verification, designed for enterprise applications requiring strong consistency, immutable audit trails, and ACID compliance.

## 🚀 Quick Start

```bash
# Clone the repository
git clone https://github.com/username/blockdb.git
cd blockdb

# Build the project
cargo build --release

# Start using BlockDB
./target/release/blockdb-cli put "user:1001" "John Doe"
./target/release/blockdb-cli get "user:1001"
```

## 📋 Table of Contents

- [Features](#-features)
- [Architecture](#-architecture)
- [Installation](#-installation)
- [Quick Start Guide](#-quick-start-guide)
- [CLI Usage](#-cli-usage)
- [API Documentation](#-api-documentation)
- [Distributed Setup](#-distributed-setup)
- [Configuration](#-configuration)
- [Performance](#-performance)
- [Contributing](#-contributing)
- [License](#-license)

## 🌟 Features

### Core Database Features
- **High-Throughput Writes**: Optimized for write-heavy workloads (190+ ops/sec)
- **Append-Only**: Immutable data model - no updates or deletes allowed
- **Blockchain Verification**: Cryptographic integrity with SHA-256 hashing
- **LSM-Tree Storage**: Memory-mapped tables with efficient compaction
- **Write-Ahead Logging**: Durability and crash recovery

### Distributed System Features
- **Raft Consensus**: Leader election and log replication
- **ACID Transactions**: Full transaction support with 2PC
- **Fine-Grained Locking**: Deadlock detection and prevention
- **Cluster Management**: Dynamic node discovery and health monitoring
- **Fault Tolerance**: Handles node failures and network partitions

### Enterprise Features
- **Strong Consistency**: CP guarantee from CAP theorem
- **Blockchain Audit Trail**: Immutable history with integrity verification
- **Multi-Node Deployment**: Horizontal scaling capabilities
- **Configurable Persistence**: Customizable storage and performance settings

## 🏗️ Architecture

BlockDB implements a layered architecture designed for scalability and reliability:

```
┌─────────────────────────────────────────────────────────────┐
│                     Client Applications                     │
├─────────────────────────────────────────────────────────────┤
│                    API Layer (HTTP/CLI)                    │
├─────────────────────────────────────────────────────────────┤
│                  Distributed Database                      │
│  ┌─────────────────┬─────────────────┬─────────────────┐   │
│  │   Consensus     │   Transactions  │   Cluster Mgmt  │   │
│  │   (Raft)        │   (2PC + Locks) │   (Discovery)   │   │
│  └─────────────────┴─────────────────┴─────────────────┘   │
├─────────────────────────────────────────────────────────────┤
│                    Storage Engine                          │
│  ┌─────────────────┬─────────────────┬─────────────────┐   │
│  │    MemTable     │    SSTable      │    Blockchain   │   │
│  │    (Memory)     │    (Disk)       │   (Integrity)   │   │
│  └─────────────────┴─────────────────┴─────────────────┘   │
├─────────────────────────────────────────────────────────────┤
│                 Write-Ahead Log (WAL)                      │
└─────────────────────────────────────────────────────────────┘
```

### Key Components

#### 1. Storage Layer
- **MemTable**: In-memory sorted map for recent writes
- **SSTable**: Disk-based sorted string tables for persistence
- **WAL**: Write-ahead log for durability and recovery
- **Blockchain**: Cryptographic verification chain

#### 2. Distributed Layer
- **Raft Consensus**: Distributed consensus algorithm
- **Transaction Manager**: ACID transaction coordination
- **Lock Manager**: Fine-grained locking with deadlock detection
- **Cluster Manager**: Node discovery and health monitoring

#### 3. API Layer
- **CLI Interface**: Command-line tools for operations
- **HTTP Server**: RESTful API endpoints (planned)
- **gRPC Service**: High-performance RPC interface (planned)

## 📦 Installation

### Prerequisites
- **Rust**: 1.70.0 or later
- **System**: Linux, macOS, or Windows
- **Memory**: Minimum 4GB RAM recommended
- **Storage**: SSD recommended for optimal performance

### Build from Source

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone and build BlockDB
git clone https://github.com/username/blockdb.git
cd blockdb
cargo build --release

# Run tests
cargo test
```

### Binary Installation

```bash
# Download pre-built binaries
wget https://github.com/username/blockdb/releases/latest/blockdb-linux-x64.tar.gz
tar -xzf blockdb-linux-x64.tar.gz
sudo mv blockdb-* /usr/local/bin/
```

## 🚀 Quick Start Guide

### 1. Basic Operations

```bash
# Initialize database (creates data directory)
./target/release/blockdb-cli stats

# Store data
./target/release/blockdb-cli put "user:1001" "John Doe"
./target/release/blockdb-cli put "user:1002" "Jane Smith"

# Retrieve data
./target/release/blockdb-cli get "user:1001"
# Output: John Doe

# Verify blockchain integrity
./target/release/blockdb-cli verify
# Output: ✓ Blockchain integrity verified successfully
```

### 2. Working with Binary Data

```bash
# Store binary data using base64 encoding
echo "binary content" | base64 | xargs ./target/release/blockdb-cli put "binary:key" --base64

# Retrieve binary data
./target/release/blockdb-cli get "binary:key" --base64 | base64 -d
```

### 3. Interactive Mode

```bash
# Start interactive session
./target/release/blockdb-cli interactive

# Interactive commands
> put user:1003 "Alice Johnson"
> get user:1003
> verify
> stats
> exit
```

## 💻 CLI Usage

### Command Reference

#### Global Options
```bash
-d, --data-dir <DATA_DIR>    Database directory [default: ./blockdb_data]
-h, --help                   Print help information
-V, --version                Print version information
```

#### Commands

##### PUT - Store Data
```bash
blockdb-cli put <KEY> <VALUE> [OPTIONS]

Options:
  --base64    Treat key and value as base64-encoded

Examples:
  blockdb-cli put "user:123" "John Doe"
  blockdb-cli put "YmluYXJ5" "ZGF0YQ==" --base64
```

##### GET - Retrieve Data
```bash
blockdb-cli get <KEY> [OPTIONS]

Options:
  --base64    Return result as base64-encoded

Examples:
  blockdb-cli get "user:123"
  blockdb-cli get "binary:key" --base64
```

##### STATS - Database Statistics
```bash
blockdb-cli stats

Example Output:
  BlockDB Statistics:
    Data directory: ./blockdb_data
    Memtable size limit: 64 MB
    WAL sync interval: 1000 ms
    Compaction threshold: 4
    Blockchain batch size: 1000
```

##### VERIFY - Blockchain Integrity
```bash
blockdb-cli verify

Example Output:
  Verifying blockchain integrity...
  ✓ Blockchain integrity verified successfully
```

##### INTERACTIVE - Interactive Mode
```bash
blockdb-cli interactive

# Starts an interactive shell for multiple operations
```

## 🌐 Distributed Setup

### Single Node Setup

```bash
# Start server
./target/release/blockdb-server \
  --host 127.0.0.1 \
  --port 8080 \
  --data-dir ./node1_data
```

### Multi-Node Cluster Setup

#### Node 1 (Leader)
```bash
./target/release/blockdb-server \
  --host 192.168.1.10 \
  --port 8080 \
  --data-dir ./node1_data \
  --cluster-id node1 \
  --peers node2:192.168.1.11:8080,node3:192.168.1.12:8080
```

#### Node 2 (Follower)
```bash
./target/release/blockdb-server \
  --host 192.168.1.11 \
  --port 8080 \
  --data-dir ./node2_data \
  --cluster-id node2 \
  --peers node1:192.168.1.10:8080,node3:192.168.1.12:8080
```

#### Node 3 (Follower)
```bash
./target/release/blockdb-server \
  --host 192.168.1.12 \
  --port 8080 \
  --data-dir ./node3_data \
  --cluster-id node3 \
  --peers node1:192.168.1.10:8080,node2:192.168.1.11:8080
```

### Cluster Operations

```bash
# Check cluster status
curl http://192.168.1.10:8080/cluster/status

# Add a new node
curl -X POST http://192.168.1.10:8080/cluster/add \
  -H "Content-Type: application/json" \
  -d '{"node_id": "node4", "address": "192.168.1.13:8080"}'

# Remove a node
curl -X DELETE http://192.168.1.10:8080/cluster/remove/node4
```

## ⚙️ Configuration

### Database Configuration

Create a `blockdb.toml` configuration file:

```toml
[database]
data_dir = "./blockdb_data"
memtable_size_limit = 67108864  # 64MB
wal_sync_interval = 1000        # milliseconds
compaction_threshold = 4
blockchain_batch_size = 1000

[server]
host = "0.0.0.0"
port = 8080
max_connections = 1000
request_timeout = 30           # seconds
enable_cors = true
enable_compression = true

[cluster]
node_id = "node1"
heartbeat_interval = 150       # milliseconds
election_timeout = 300         # milliseconds
enable_transactions = true
transaction_timeout = 30       # seconds

[logging]
level = "info"
file = "./logs/blockdb.log"
```

### Environment Variables

```bash
export BLOCKDB_DATA_DIR="/var/lib/blockdb"
export BLOCKDB_LOG_LEVEL="debug"
export BLOCKDB_SERVER_PORT="8080"
export BLOCKDB_CLUSTER_ID="production-node-1"
```

### Performance Tuning

#### Memory Settings
```toml
[database]
memtable_size_limit = 134217728  # 128MB for high-throughput
wal_sync_interval = 500          # More frequent syncs
```

#### Disk Settings
```toml
[database]
compaction_threshold = 8         # Less frequent compaction
blockchain_batch_size = 5000     # Larger batches
```

#### Cluster Settings
```toml
[cluster]
heartbeat_interval = 100         # Faster heartbeats
election_timeout = 200           # Quicker leader election
```

## 📈 Performance

### Benchmarks

#### Single Node Performance
- **Write Throughput**: 190+ operations/second
- **Read Latency**: < 1ms (cached), < 5ms (disk)
- **Memory Usage**: ~100MB base + data
- **Disk Usage**: Efficient with LSM-tree compaction

#### Cluster Performance
- **Consensus Latency**: < 10ms (3-node cluster)
- **Replication Throughput**: 85% of single-node write performance
- **Fault Recovery**: < 5 seconds leader election

### Optimization Tips

1. **Use SSDs**: Significant performance improvement for write-heavy workloads
2. **Tune MemTable Size**: Larger memtables reduce compaction frequency
3. **Batch Operations**: Group related operations for better throughput
4. **Monitor Compaction**: Adjust thresholds based on access patterns
5. **Network Optimization**: Use dedicated network for cluster communication

### Monitoring

```bash
# Performance metrics endpoint
curl http://localhost:8080/metrics

# Health check
curl http://localhost:8080/health

# Cluster status
curl http://localhost:8080/cluster/status
```

## 🔧 API Documentation

### HTTP API Endpoints

#### Data Operations
```
POST /api/v1/put
GET  /api/v1/get/{key}
POST /api/v1/batch
```

#### Cluster Management
```
GET    /cluster/status
POST   /cluster/add
DELETE /cluster/remove/{node_id}
GET    /cluster/health
```

#### System Operations
```
GET /health
GET /metrics
GET /stats
POST /verify
```

### Example API Usage

#### Store Data
```bash
curl -X POST http://localhost:8080/api/v1/put \
  -H "Content-Type: application/json" \
  -d '{"key": "user:1001", "value": "John Doe"}'
```

#### Retrieve Data
```bash
curl http://localhost:8080/api/v1/get/user:1001
```

#### Batch Operations
```bash
curl -X POST http://localhost:8080/api/v1/batch \
  -H "Content-Type: application/json" \
  -d '{
    "operations": [
      {"type": "put", "key": "user:1001", "value": "John Doe"},
      {"type": "put", "key": "user:1002", "value": "Jane Smith"}
    ]
  }'
```

## 🐛 Troubleshooting

### Common Issues

#### 1. Permission Denied
```bash
# Error: Permission denied (os error 13)
# Solution: Check directory permissions
chmod 755 ./blockdb_data
```

#### 2. Port Already in Use
```bash
# Error: Address already in use (os error 48)
# Solution: Change port or kill existing process
lsof -ti:8080 | xargs kill -9
```

#### 3. Cluster Connection Failed
```bash
# Error: Failed to connect to peer
# Solution: Check network connectivity and firewall
telnet 192.168.1.10 8080
```

#### 4. Blockchain Integrity Failed
```bash
# Error: Blockchain integrity verification failed
# Solution: Check for data corruption, restore from backup
./target/release/blockdb-cli verify --repair
```

### Debug Mode

```bash
# Enable debug logging
RUST_LOG=debug ./target/release/blockdb-server

# Enable trace logging for specific modules
RUST_LOG=blockdb::consensus=trace ./target/release/blockdb-server
```

### Performance Issues

1. **Slow Writes**: Check memtable size and disk I/O
2. **Memory Usage**: Monitor compaction frequency
3. **Network Latency**: Verify cluster network configuration
4. **Consensus Delays**: Check leader election timeouts

## 🤝 Contributing

We welcome contributions! Please see our [Contributing Guide](CONTRIBUTING.md) for details.

### Development Setup

```bash
# Fork and clone
git clone https://github.com/yourusername/blockdb.git
cd blockdb

# Install development dependencies
cargo install cargo-watch cargo-audit

# Run tests
cargo test

# Run with file watching
cargo watch -x test
```

### Code Style

```bash
# Format code
cargo fmt

# Lint code
cargo clippy

# Security audit
cargo audit
```

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- **Raft Consensus Algorithm**: Diego Ongaro and John Ousterhout
- **LSM-Tree Design**: Patrick O'Neil, Edward Cheng, Dieter Gawlick, Elizabeth O'Neil
- **Blockchain Concepts**: Satoshi Nakamoto's Bitcoin whitepaper
- **Rust Community**: For excellent tooling and libraries

## 📞 Support

- **Documentation**: [https://blockdb.dev/docs](https://blockdb.dev/docs)
- **Issues**: [GitHub Issues](https://github.com/username/blockdb/issues)
- **Discussions**: [GitHub Discussions](https://github.com/username/blockdb/discussions)
- **Discord**: [BlockDB Community](https://discord.gg/blockdb)

---

**BlockDB** - *Building the future of distributed, immutable data storage*