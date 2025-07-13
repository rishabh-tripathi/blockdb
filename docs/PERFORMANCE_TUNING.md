# BlockDB Performance Tuning Guide

## Overview

This guide provides comprehensive performance optimization strategies for BlockDB, covering system-level, database-level, and application-level tuning techniques.

## Table of Contents

- [Performance Fundamentals](#performance-fundamentals)
- [System-Level Optimization](#system-level-optimization)
- [Database Configuration Tuning](#database-configuration-tuning)
- [Storage Optimization](#storage-optimization)
- [Network Optimization](#network-optimization)
- [Application-Level Optimization](#application-level-optimization)
- [Monitoring and Profiling](#monitoring-and-profiling)
- [Benchmarking](#benchmarking)

## Performance Fundamentals

### Understanding BlockDB Performance Characteristics

BlockDB is optimized for:
- **High-throughput writes** (append-only workloads)
- **Sequential I/O patterns** (LSM-tree architecture)
- **Strong consistency** (distributed consensus)
- **Low-latency reads** (memory-first architecture)

### Key Performance Metrics

| Metric | Target (Single Node) | Target (3-Node Cluster) |
|--------|---------------------|-------------------------|
| Write Throughput | 200+ ops/sec | 150+ ops/sec |
| Read Latency | < 5ms | < 10ms |
| Write Latency | < 20ms | < 50ms |
| Memory Usage | < 1GB | < 1GB per node |
| Consensus Latency | N/A | < 20ms |

### Performance Bottlenecks

Common bottlenecks in order of impact:
1. **Disk I/O** - Storage speed affects write performance
2. **Network Latency** - Critical for distributed consensus
3. **Memory** - MemTable size affects flush frequency
4. **CPU** - Consensus and cryptographic operations
5. **Lock Contention** - Transaction conflicts

## System-Level Optimization

### Hardware Recommendations

#### Storage

**Optimal Configuration:**
```bash
# NVMe SSD with high IOPS
Device: NVMe SSD
Capacity: 1TB+
IOPS: 10,000+ (4KB random writes)
Latency: < 0.1ms
Interface: PCIe 3.0 x4 or better
```

**Filesystem Tuning:**
```bash
# Use ext4 with optimal mount options
/dev/nvme0n1 /var/lib/blockdb ext4 noatime,nodiratime,data=writeback,barrier=0 0 0

# XFS alternative (better for large files)
/dev/nvme0n1 /var/lib/blockdb xfs noatime,nodiratime,logbsize=256k,largeio 0 0

# Disable disk barriers for better performance (if UPS protected)
echo 0 > /sys/block/nvme0n1/queue/add_random
echo deadline > /sys/block/nvme0n1/queue/scheduler
```

#### Memory

**Configuration:**
```bash
# Recommended memory allocation
Total RAM: 16GB+
OS/System: 4GB
BlockDB Process: 4GB
MemTable: 2GB
Page Cache: 4GB
Buffer: 2GB
```

**Memory Tuning:**
```bash
# Optimize memory management
cat >> /etc/sysctl.conf << EOF
# Memory management
vm.swappiness = 1                    # Avoid swap usage
vm.dirty_ratio = 15                  # Dirty page ratio
vm.dirty_background_ratio = 5        # Background dirty pages
vm.dirty_writeback_centisecs = 500   # Writeback frequency
vm.dirty_expire_centisecs = 3000     # Page expiration
vm.vfs_cache_pressure = 50           # Cache pressure
EOF

sysctl -p
```

#### CPU

**Optimization:**
```bash
# CPU governor for performance
echo performance > /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor

# Disable CPU idle states for consistent latency
for i in /sys/devices/system/cpu/cpu*/cpuidle/state*/disable; do
    echo 1 > $i 2>/dev/null || true
done

# Set CPU affinity (if needed)
systemctl edit blockdb
# Add:
# [Service]
# ExecStart=
# ExecStart=taskset -c 0-3 /usr/local/bin/blockdb-server --config /etc/blockdb/blockdb.toml
```

#### Network

**System-level Network Tuning:**
```bash
cat >> /etc/sysctl.conf << EOF
# Network optimizations
net.core.rmem_max = 16777216
net.core.wmem_max = 16777216
net.core.rmem_default = 262144
net.core.wmem_default = 262144
net.ipv4.tcp_rmem = 4096 12582912 16777216
net.ipv4.tcp_wmem = 4096 12582912 16777216
net.core.netdev_max_backlog = 5000
net.core.netdev_budget = 600
net.ipv4.tcp_congestion_control = bbr
net.ipv4.tcp_slow_start_after_idle = 0
EOF

sysctl -p
```

### File System Optimization

#### Disk Scheduler

```bash
# Use deadline scheduler for SSDs
echo deadline > /sys/block/nvme0n1/queue/scheduler

# For HDDs, use CFQ
echo cfq > /sys/block/sda/queue/scheduler

# Optimize queue depth
echo 32 > /sys/block/nvme0n1/queue/nr_requests
```

#### File Descriptor Limits

```bash
# Increase file descriptor limits
cat >> /etc/security/limits.conf << EOF
blockdb soft nofile 65536
blockdb hard nofile 65536
blockdb soft nproc 32768
blockdb hard nproc 32768
EOF

# For systemd services
mkdir -p /etc/systemd/system/blockdb.service.d
cat > /etc/systemd/system/blockdb.service.d/limits.conf << EOF
[Service]
LimitNOFILE=65536
LimitNPROC=32768
EOF

systemctl daemon-reload
```

## Database Configuration Tuning

### Write Performance Optimization

#### MemTable Configuration

```toml
[database]
# Larger MemTable reduces flush frequency
memtable_size_limit = 268435456      # 256MB (up from 64MB default)

# Adjust based on available memory:
# - 8GB RAM:  128MB MemTable
# - 16GB RAM: 256MB MemTable  
# - 32GB RAM: 512MB MemTable
```

**Impact Analysis:**
- **Larger MemTable**: Fewer flushes, better write throughput, higher memory usage
- **Smaller MemTable**: More frequent flushes, lower memory usage, potential I/O spikes

#### WAL Configuration

```toml
[database]
# Adjust sync frequency based on durability requirements
wal_sync_interval = 1000             # 1 second (default)
# wal_sync_interval = 5000           # 5 seconds for higher throughput
# wal_sync_interval = 100            # 100ms for lower latency

# WAL file size (if configurable)
wal_file_size = 134217728            # 128MB
```

**Sync Interval Guidelines:**
- **High Durability**: 100-500ms
- **Balanced**: 1000ms (default)
- **High Throughput**: 2000-5000ms

#### Compaction Tuning

```toml
[database]
# Adjust compaction threshold
compaction_threshold = 8             # Up from 4 (fewer compactions)

# Compaction strategy (if configurable)
compaction_strategy = "size_tiered"  # or "leveled"
max_compaction_threads = 4           # Parallel compaction
```

### Read Performance Optimization

#### Caching Configuration

```toml
[database]
# Index cache size
index_cache_size = 134217728         # 128MB

# Block cache size  
block_cache_size = 268435456         # 256MB

# Enable bloom filters (reduces disk reads)
enable_bloom_filters = true
bloom_filter_bits_per_key = 10
```

#### SSTable Optimization

```toml
[database]
# SSTable block size
sstable_block_size = 32768          # 32KB (larger blocks for sequential reads)

# Index block size
index_block_size = 4096             # 4KB

# Compression (if available)
compression_type = "snappy"         # or "lz4", "zstd"
compression_level = 3
```

### Blockchain Performance

```toml
[database]
# Larger batch size reduces blockchain overhead
blockchain_batch_size = 5000        # Up from 1000

# Disable verification for performance testing (NOT for production)
# enable_blockchain_verification = false
```

### Distributed Configuration

#### Consensus Tuning

```toml
[cluster]
# Aggressive timeouts for low-latency networks
heartbeat_interval = 50             # 50ms (down from 150ms)
election_timeout = 150              # 150ms (down from 300ms)

# Conservative timeouts for high-latency networks
# heartbeat_interval = 500          # 500ms
# election_timeout = 1500           # 1.5 seconds
```

#### Transaction Configuration

```toml
[cluster]
# Transaction timeouts
transaction_timeout = 10            # 10 seconds for faster timeout
enable_transactions = true

# 2PC optimization
two_pc_timeout = 5                  # 5 seconds
```

## Storage Optimization

### Disk Layout

#### Separate WAL and Data

```bash
# Mount WAL on separate fast disk
mkdir -p /var/lib/blockdb/wal
mount /dev/nvme1n1 /var/lib/blockdb/wal

# Update configuration
[database]
data_dir = "/var/lib/blockdb"
wal_dir = "/var/lib/blockdb/wal"      # Separate WAL directory
```

#### RAID Configuration

```bash
# RAID 0 for maximum performance (no redundancy)
mdadm --create /dev/md0 --level=0 --raid-devices=2 /dev/nvme0n1 /dev/nvme1n1

# RAID 10 for performance + redundancy
mdadm --create /dev/md0 --level=10 --raid-devices=4 /dev/nvme{0..3}n1
```

### I/O Optimization

#### I/O Scheduler Tuning

```bash
# For NVMe drives
echo none > /sys/block/nvme0n1/queue/scheduler

# For SATA SSDs  
echo deadline > /sys/block/sda/queue/scheduler

# Disable NCQ for consistent latency
echo 1 > /sys/block/sda/queue/nomerges
```

#### Direct I/O Configuration

```toml
[database]
# Enable direct I/O (bypasses page cache)
enable_direct_io = true              # Reduces double buffering

# Write buffer size
write_buffer_size = 1048576          # 1MB write buffer
```

### Space Management

#### Automatic Cleanup

```bash
#!/bin/bash
# /usr/local/bin/blockdb-cleanup.sh

# Clean old WAL files
find /var/lib/blockdb -name "*.wal.old" -mtime +7 -delete

# Compact if needed
SSTABLE_COUNT=$(ls /var/lib/blockdb/*.sst 2>/dev/null | wc -l)
if [ $SSTABLE_COUNT -gt 10 ]; then
    blockdb-cli compact
fi

# Clean old logs
find /var/log/blockdb -name "*.log.*" -mtime +30 -delete
```

## Network Optimization

### Cluster Network Tuning

#### TCP Optimization

```toml
[server]
# TCP socket options
tcp_nodelay = true                   # Disable Nagle's algorithm
tcp_keepalive = true                 # Enable keepalive
keepalive_time = 30                  # 30 seconds
keepalive_interval = 5               # 5 seconds
keepalive_probes = 3                 # 3 probes

# Buffer sizes
send_buffer_size = 1048576           # 1MB send buffer
recv_buffer_size = 1048576           # 1MB receive buffer
```

#### Connection Management

```toml
[server]
# Connection limits
max_connections = 5000               # Increase connection limit
connection_timeout = 60              # 60 seconds
max_idle_time = 300                  # 5 minutes

# Worker threads
worker_threads = 8                   # Match CPU cores
```

### Load Balancing

#### HAProxy Configuration

```
backend blockdb_cluster
    balance roundrobin
    option tcp-check
    
    # Connection tuning
    timeout connect 1s
    timeout server 30s
    
    # Health checks
    option httpchk GET /health
    http-check expect status 200
    
    server node1 192.168.1.10:8080 check inter 2s rise 3 fall 2
    server node2 192.168.1.11:8080 check inter 2s rise 3 fall 2
    server node3 192.168.1.12:8080 check inter 2s rise 3 fall 2
```

## Application-Level Optimization

### Client Configuration

#### Connection Pooling

```rust
// Rust client example
use blockdb_client::{ClientConfig, ConnectionPool};

let config = ClientConfig {
    max_connections: 20,
    connection_timeout: Duration::from_secs(5),
    idle_timeout: Duration::from_secs(300),
    retry_attempts: 3,
    retry_delay: Duration::from_millis(100),
};

let pool = ConnectionPool::new(config);
```

#### Batch Operations

```rust
// Batch multiple operations for better throughput
let batch = vec![
    ("key1", "value1"),
    ("key2", "value2"), 
    ("key3", "value3"),
];

client.batch_put(batch).await?;
```

### Query Optimization

#### Key Design

```rust
// Good: Hierarchical keys for locality
"user:1001:profile"
"user:1001:settings"
"user:1001:history"

// Bad: Random keys
"a7f3k9m2"
"x9z4p1q8"
```

#### Value Size Optimization

```rust
// Optimal value sizes: 1KB - 100KB
// Too small: High overhead
// Too large: Memory pressure

if value.len() > 1024 * 1024 {  // 1MB
    // Consider splitting large values
    split_value_into_chunks(value);
}
```

## Monitoring and Profiling

### Performance Metrics

#### Key Metrics to Monitor

```bash
# System metrics
iostat -x 1                         # I/O statistics
sar -n DEV 1                        # Network statistics  
free -h                             # Memory usage
top -p $(pgrep blockdb-server)      # Process statistics

# BlockDB metrics
curl http://localhost:8080/metrics  # Application metrics
blockdb-cli stats                   # Database statistics
```

#### Custom Monitoring Script

```bash
#!/bin/bash
# /usr/local/bin/blockdb-perf-monitor.sh

INTERVAL=5
LOG_FILE="/var/log/blockdb-performance.log"

while true; do
    TIMESTAMP=$(date '+%Y-%m-%d %H:%M:%S')
    
    # Get BlockDB PID
    PID=$(pgrep blockdb-server)
    
    if [ -n "$PID" ]; then
        # CPU and Memory
        CPU_MEM=$(ps -o %cpu,%mem,rss -p $PID --no-headers)
        
        # I/O stats
        IO_STATS=$(cat /proc/$PID/io | grep -E "read_bytes|write_bytes" | awk '{print $2}' | paste -sd,)
        
        # Network connections
        CONNECTIONS=$(ss -tn | grep :8080 | wc -l)
        
        # File descriptors
        FD_COUNT=$(ls /proc/$PID/fd | wc -l)
        
        # Disk usage
        DISK_USAGE=$(df /var/lib/blockdb | tail -1 | awk '{print $5}')
        
        echo "$TIMESTAMP,$CPU_MEM,$IO_STATS,$CONNECTIONS,$FD_COUNT,$DISK_USAGE" >> $LOG_FILE
    fi
    
    sleep $INTERVAL
done
```

### Profiling Tools

#### CPU Profiling

```bash
# Use perf for CPU profiling
sudo perf record -g -p $(pgrep blockdb-server) -- sleep 30
sudo perf report

# Flame graph generation
git clone https://github.com/brendangregg/FlameGraph
sudo perf record -g -p $(pgrep blockdb-server) -- sleep 30
sudo perf script | ./FlameGraph/stackcollapse-perf.pl | ./FlameGraph/flamegraph.pl > blockdb-cpu.svg
```

#### Memory Profiling

```bash
# Use valgrind for memory analysis
valgrind --tool=massif --pages-as-heap=yes blockdb-server --config test.toml

# Heap profiling with jemalloc
export MALLOC_CONF="prof:true,prof_active:true"
blockdb-server --config blockdb.toml
```

## Benchmarking

### Benchmark Scripts

#### Write Performance Test

```bash
#!/bin/bash
# write_benchmark.sh

OPERATIONS=10000
CONCURRENCY=10
VALUE_SIZE=1024

echo "Starting write benchmark..."
echo "Operations: $OPERATIONS"
echo "Concurrency: $CONCURRENCY"
echo "Value size: $VALUE_SIZE bytes"

# Generate test data
openssl rand -hex $VALUE_SIZE > test_value.txt
TEST_VALUE=$(cat test_value.txt)

# Benchmark function
benchmark_writes() {
    local thread_id=$1
    local ops_per_thread=$((OPERATIONS / CONCURRENCY))
    
    for i in $(seq 1 $ops_per_thread); do
        key="thread_${thread_id}_key_${i}"
        blockdb-cli put "$key" "$TEST_VALUE" > /dev/null 2>&1
    done
}

# Start benchmark
START_TIME=$(date +%s.%N)

# Launch concurrent writers
for i in $(seq 1 $CONCURRENCY); do
    benchmark_writes $i &
done

# Wait for completion
wait

END_TIME=$(date +%s.%N)
DURATION=$(echo "$END_TIME - $START_TIME" | bc)
THROUGHPUT=$(echo "scale=2; $OPERATIONS / $DURATION" | bc)

echo "Duration: ${DURATION}s"
echo "Throughput: ${THROUGHPUT} ops/sec"

# Cleanup
rm test_value.txt
```

#### Read Performance Test

```bash
#!/bin/bash
# read_benchmark.sh

OPERATIONS=10000
CONCURRENCY=10

# Populate data first
echo "Populating test data..."
for i in $(seq 1 $OPERATIONS); do
    blockdb-cli put "benchmark_key_$i" "benchmark_value_$i" > /dev/null 2>&1
done

echo "Starting read benchmark..."

benchmark_reads() {
    local thread_id=$1
    local ops_per_thread=$((OPERATIONS / CONCURRENCY))
    
    for i in $(seq 1 $ops_per_thread); do
        key="benchmark_key_$((RANDOM % OPERATIONS + 1))"
        blockdb-cli get "$key" > /dev/null 2>&1
    done
}

START_TIME=$(date +%s.%N)

for i in $(seq 1 $CONCURRENCY); do
    benchmark_reads $i &
done

wait

END_TIME=$(date +%s.%N)
DURATION=$(echo "$END_TIME - $START_TIME" | bc)
THROUGHPUT=$(echo "scale=2; $OPERATIONS / $DURATION" | bc)

echo "Read Duration: ${DURATION}s"
echo "Read Throughput: ${THROUGHPUT} ops/sec"
```

### Load Testing

#### HTTP API Load Test

```bash
# Using Apache Bench
ab -n 10000 -c 100 -p post_data.json -T application/json http://localhost:8080/api/v1/put

# Using wrk
wrk -t4 -c100 -d30s -s post_script.lua http://localhost:8080/api/v1/put
```

### Distributed Benchmark

```bash
#!/bin/bash
# distributed_benchmark.sh

NODES=("node1:8080" "node2:8080" "node3:8080")
OPERATIONS=30000
CONCURRENCY=30

echo "Starting distributed benchmark across ${#NODES[@]} nodes..."

benchmark_node() {
    local node=$1
    local ops_per_node=$((OPERATIONS / ${#NODES[@]}))
    
    for i in $(seq 1 $ops_per_node); do
        key="distributed_key_${node}_${i}"
        value="distributed_value_${i}"
        
        curl -s -X POST http://$node/api/v1/put \
             -H "Content-Type: application/json" \
             -d "{\"key\":\"$key\",\"value\":\"$value\"}" > /dev/null
    done
}

START_TIME=$(date +%s.%N)

for node in "${NODES[@]}"; do
    benchmark_node $node &
done

wait

END_TIME=$(date +%s.%N)
DURATION=$(echo "$END_TIME - $START_TIME" | bc)
THROUGHPUT=$(echo "scale=2; $OPERATIONS / $DURATION" | bc)

echo "Distributed Duration: ${DURATION}s"  
echo "Distributed Throughput: ${THROUGHPUT} ops/sec"

# Verify cluster consistency
echo "Verifying cluster consistency..."
for node in "${NODES[@]}"; do
    echo "Node $node status:"
    curl -s http://$node/cluster/status | jq '.state,.leader'
done
```

## Performance Tuning Checklist

### Pre-Production Checklist

- [ ] **Hardware**
  - [ ] SSD storage (NVMe preferred)
  - [ ] Sufficient RAM (16GB+ recommended)
  - [ ] Fast network (1Gbps+ for clusters)
  - [ ] Multiple CPU cores

- [ ] **System Configuration**
  - [ ] File descriptor limits increased
  - [ ] Memory management optimized
  - [ ] Network stack tuned
  - [ ] Disk scheduler optimized

- [ ] **Database Configuration**
  - [ ] MemTable size optimized for workload
  - [ ] WAL sync interval tuned
  - [ ] Compaction threshold adjusted
  - [ ] Blockchain batch size optimized

- [ ] **Cluster Configuration**
  - [ ] Heartbeat/election timeouts tuned for network
  - [ ] Transaction timeouts set appropriately
  - [ ] Load balancer configured
  - [ ] Monitoring enabled

- [ ] **Application Optimization**
  - [ ] Connection pooling implemented
  - [ ] Batch operations used where possible
  - [ ] Key design optimized
  - [ ] Value sizes reasonable

- [ ] **Monitoring**
  - [ ] Performance metrics collection
  - [ ] Alerting configured
  - [ ] Capacity planning done
  - [ ] Benchmarking completed

This performance tuning guide provides a comprehensive approach to optimizing BlockDB for various workloads and environments.