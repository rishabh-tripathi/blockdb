# BlockDB Troubleshooting Guide

## Overview

This guide helps diagnose and resolve common issues with BlockDB deployments, from simple configuration problems to complex distributed system failures.

## Table of Contents

- [General Troubleshooting](#general-troubleshooting)
- [Installation Issues](#installation-issues)
- [Configuration Problems](#configuration-problems)
- [Performance Issues](#performance-issues)
- [Cluster Issues](#cluster-issues)
- [Data Integrity Issues](#data-integrity-issues)
- [Monitoring and Diagnostics](#monitoring-and-diagnostics)
- [Recovery Procedures](#recovery-procedures)

## General Troubleshooting

### Step-by-Step Diagnostic Process

1. **Identify Symptoms**: What exactly is failing?
2. **Check Logs**: Look for error messages and warnings
3. **Verify Configuration**: Ensure settings are correct
4. **Test Connectivity**: Network, disk, and service availability
5. **Check Resources**: CPU, memory, disk space
6. **Isolate the Problem**: Single node vs. cluster-wide

### Essential Commands

```bash
# Check service status
systemctl status blockdb
journalctl -u blockdb -f

# Test basic functionality
blockdb-cli stats
blockdb-cli verify

# Check resource usage
top -p $(pgrep blockdb-server)
df -h /var/lib/blockdb
ss -tulpn | grep 8080

# View logs with timestamps
tail -f /var/log/blockdb/blockdb.log | while read line; do echo "$(date): $line"; done
```

### Log Level Configuration

Enable detailed logging for troubleshooting:

```toml
[logging]
level = "debug"  # or "trace" for maximum verbosity
file = "/var/log/blockdb/blockdb.log"
```

```bash
# Runtime log level change
RUST_LOG=debug blockdb-server --config /etc/blockdb/blockdb.toml
```

## Installation Issues

### Binary Installation Problems

#### Issue: Permission Denied

**Symptoms:**
```bash
$ ./blockdb-cli --version
bash: ./blockdb-cli: Permission denied
```

**Solutions:**
```bash
# Fix executable permissions
chmod +x blockdb-cli blockdb-server

# Check file ownership
ls -la blockdb-*
chown $USER:$USER blockdb-*
```

#### Issue: Library Dependencies Missing

**Symptoms:**
```bash
$ ./blockdb-server
error while loading shared libraries: libssl.so.1.1: cannot open shared object file
```

**Solutions:**
```bash
# Ubuntu/Debian
sudo apt-get update
sudo apt-get install libssl1.1 libssl-dev

# CentOS/RHEL
sudo yum install openssl-libs openssl-devel

# Check dependencies
ldd blockdb-server
```

### Source Compilation Issues

#### Issue: Rust Version Too Old

**Symptoms:**
```bash
error: package `blockdb v0.1.0` cannot be compiled due to multiple conflicting dependencies
```

**Solutions:**
```bash
# Update Rust
rustup update stable
rustc --version  # Should be 1.70.0 or later

# Clean and rebuild
cargo clean
cargo build --release
```

#### Issue: Compilation Errors

**Symptoms:**
```bash
error[E0425]: cannot find function `foo` in this scope
```

**Solutions:**
```bash
# Update dependencies
cargo update

# Check for feature flags
cargo check --features "all"

# Verbose build for details
cargo build --release --verbose
```

## Configuration Problems

### File Permissions

#### Issue: Cannot Create Data Directory

**Symptoms:**
```bash
Error: IoError(Os { code: 13, kind: PermissionDenied, message: "Permission denied" })
```

**Solutions:**
```bash
# Create directory with correct permissions
sudo mkdir -p /var/lib/blockdb
sudo chown blockdb:blockdb /var/lib/blockdb
sudo chmod 755 /var/lib/blockdb

# Check SELinux context (if applicable)
ls -Z /var/lib/blockdb
sudo setsebool -P allow_httpd_anon_write 1
```

### Configuration File Issues

#### Issue: Invalid TOML Syntax

**Symptoms:**
```bash
Error: Config error: invalid TOML syntax at line 15
```

**Solutions:**
```bash
# Validate TOML syntax
python3 -c "import toml; toml.load('/etc/blockdb/blockdb.toml')"

# Check common syntax errors
cat -n /etc/blockdb/blockdb.toml | grep -E "(=|\"|\[)"

# Use default config as template
blockdb-server --print-default-config > blockdb.toml
```

#### Issue: Port Already in Use

**Symptoms:**
```bash
Error: Address already in use (os error 48)
```

**Solutions:**
```bash
# Find process using port
sudo lsof -i :8080
sudo ss -tulpn | grep 8080

# Kill conflicting process
sudo kill -9 <PID>

# Use different port
# In config: port = 8081

# Check firewall rules
sudo ufw status
sudo iptables -L -n
```

### Resource Limits

#### Issue: Too Many Open Files

**Symptoms:**
```bash
Error: Too many open files (os error 24)
```

**Solutions:**
```bash
# Check current limits
ulimit -n
cat /proc/$(pgrep blockdb-server)/limits | grep files

# Increase limits temporarily
ulimit -n 65536

# Permanent fix in /etc/security/limits.conf
echo "blockdb soft nofile 65536" >> /etc/security/limits.conf
echo "blockdb hard nofile 65536" >> /etc/security/limits.conf

# For systemd services
mkdir -p /etc/systemd/system/blockdb.service.d
cat > /etc/systemd/system/blockdb.service.d/limits.conf << EOF
[Service]
LimitNOFILE=65536
EOF

systemctl daemon-reload
systemctl restart blockdb
```

## Performance Issues

### Slow Write Performance

#### Symptoms
- Write operations taking > 100ms
- High CPU usage during writes
- Memory usage growing continuously

#### Diagnostics

```bash
# Check write latency
curl -w "@curl-format.txt" -X POST http://localhost:8080/api/v1/put \
  -H "Content-Type: application/json" \
  -d '{"key": "test", "value": "test"}'

# Monitor I/O patterns
sudo iotop -p $(pgrep blockdb-server)
iostat -x 1

# Check memtable flush frequency
grep "Flushing memtable" /var/log/blockdb/blockdb.log | tail -20
```

#### Solutions

1. **Increase MemTable Size**:
```toml
[database]
memtable_size_limit = 134217728  # 128MB instead of 64MB
```

2. **Optimize WAL Sync**:
```toml
[database]
wal_sync_interval = 2000  # Less frequent syncing
```

3. **Use SSDs**:
```bash
# Check if using SSD
lsblk -d -o name,rota
# rota=0 means SSD, rota=1 means HDD
```

4. **Optimize Compaction**:
```toml
[database]
compaction_threshold = 8  # Delay compaction
```

### High Memory Usage

#### Symptoms
- Memory usage growing without bound
- Out of memory errors
- System becoming unresponsive

#### Diagnostics

```bash
# Monitor memory usage
ps aux | grep blockdb-server
cat /proc/$(pgrep blockdb-server)/status | grep -E "(VmRSS|VmSize)"

# Check for memory leaks
valgrind --tool=memcheck --leak-check=full blockdb-server --config test.toml

# Monitor garbage collection (if applicable)
RUST_LOG=debug blockdb-server --config blockdb.toml 2>&1 | grep -i memory
```

#### Solutions

1. **Reduce MemTable Size**:
```toml
[database]
memtable_size_limit = 33554432  # 32MB
```

2. **Increase Compaction Frequency**:
```toml
[database]
compaction_threshold = 2  # More frequent compaction
```

3. **Monitor Background Tasks**:
```bash
# Check for stuck compaction
grep "Compaction" /var/log/blockdb/blockdb.log | tail -10
```

### High Read Latency

#### Symptoms
- Read operations taking > 10ms
- Timeouts on read requests
- High disk I/O during reads

#### Diagnostics

```bash
# Test read performance
time blockdb-cli get "test_key"

# Check SSTable count
blockdb-cli stats | grep -i sstable

# Monitor cache hit rates
grep "cache" /var/log/blockdb/blockdb.log | tail -20
```

#### Solutions

1. **Reduce SSTable Count**:
```bash
# Trigger manual compaction (if available)
blockdb-cli compact
```

2. **Optimize Index Caching**:
```toml
[database]
index_cache_size = 67108864  # 64MB index cache
```

3. **Use Bloom Filters** (if implemented):
```toml
[database]
enable_bloom_filters = true
bloom_filter_bits_per_key = 10
```

## Cluster Issues

### Leader Election Problems

#### Symptoms
- No leader elected after startup
- Frequent leader changes
- Split-brain scenarios

#### Diagnostics

```bash
# Check cluster status on all nodes
for node in node1 node2 node3; do
  echo "=== $node ==="
  curl -s http://$node:8080/cluster/status | jq '.state,.leader'
done

# Check election logs
grep -i "election\|leader\|vote" /var/log/blockdb/blockdb.log | tail -20

# Check network connectivity between nodes
for node in node1 node2 node3; do
  echo "Testing connectivity to $node:"
  nc -zv $node 8080
done
```

#### Solutions

1. **Check Network Connectivity**:
```bash
# Test latency between nodes
ping -c 5 node2
traceroute node2

# Check for packet loss
mtr --report node2
```

2. **Adjust Election Timeouts**:
```toml
[cluster]
election_timeout = 500      # Increase if network is slow
heartbeat_interval = 200    # Increase if network is unreliable
```

3. **Verify Time Synchronization**:
```bash
# Check time on all nodes
date
chrony sources -v  # or ntpq -p

# Synchronize time
sudo chrony makestep
```

### Node Connectivity Issues

#### Symptoms
- Nodes showing as disconnected
- Consensus timeouts
- Failed replication

#### Diagnostics

```bash
# Check listening ports
ss -tulpn | grep 8080

# Test connectivity from other nodes
telnet node1 8080

# Check firewall rules
sudo iptables -L -n | grep 8080
sudo ufw status

# Check DNS resolution
nslookup node1
dig node1
```

#### Solutions

1. **Configure Firewall**:
```bash
# Allow BlockDB port
sudo ufw allow 8080/tcp
sudo iptables -A INPUT -p tcp --dport 8080 -j ACCEPT
```

2. **Use IP Addresses Instead of Hostnames**:
```toml
[cluster]
peers = [
    "192.168.1.11:8080",  # Instead of "node2:8080"
    "192.168.1.12:8080"   # Instead of "node3:8080"
]
```

3. **Check Network Interface Binding**:
```toml
[server]
host = "0.0.0.0"  # Listen on all interfaces instead of 127.0.0.1
```

### Consensus Timeouts

#### Symptoms
- Operations timing out
- Consensus taking too long
- Client timeouts

#### Diagnostics

```bash
# Check consensus latency
curl -w "%{time_total}" -X POST http://localhost:8080/api/v1/put \
  -H "Content-Type: application/json" \
  -d '{"key": "test", "value": "test"}'

# Check raft logs
grep "consensus\|raft\|timeout" /var/log/blockdb/blockdb.log | tail -20

# Monitor network latency
ping -c 100 node2 | tail -5
```

#### Solutions

1. **Increase Timeouts**:
```toml
[cluster]
consensus_timeout = 10000  # 10 seconds
```

2. **Optimize Network**:
```bash
# Increase network buffers
echo 'net.core.rmem_max = 16777216' >> /etc/sysctl.conf
echo 'net.core.wmem_max = 16777216' >> /etc/sysctl.conf
sysctl -p
```

## Data Integrity Issues

### Blockchain Verification Failures

#### Symptoms
```bash
$ blockdb-cli verify
✗ Blockchain integrity verification failed
Error: BlockchainError("Hash mismatch at block 42")
```

#### Diagnostics

```bash
# Check blockchain status
blockdb-cli stats | grep -i blockchain

# Look for corruption patterns
grep -i "integrity\|corrupt\|hash" /var/log/blockdb/blockdb.log

# Check disk health
sudo smartctl -a /dev/sda
sudo fsck -n /dev/sda1  # Read-only check
```

#### Solutions

1. **Restore from Backup**:
```bash
# Stop service
sudo systemctl stop blockdb

# Restore data
sudo rsync -av /backup/blockdb/ /var/lib/blockdb/

# Restart service
sudo systemctl start blockdb
```

2. **Rebuild Blockchain** (if feature available):
```bash
# Backup current data
cp -r /var/lib/blockdb /var/lib/blockdb.backup

# Rebuild blockchain from WAL
blockdb-cli rebuild-blockchain --wal-file /var/lib/blockdb/wal.log
```

### WAL Corruption

#### Symptoms
- Cannot start database
- WAL replay failures
- Data inconsistencies

#### Diagnostics

```bash
# Check WAL file integrity
ls -la /var/lib/blockdb/wal.log
file /var/lib/blockdb/wal.log

# Check for disk errors
dmesg | grep -i error
```

#### Solutions

1. **Restore from Backup**:
```bash
# Use most recent clean backup
sudo systemctl stop blockdb
sudo cp /backup/blockdb/wal.log /var/lib/blockdb/
sudo systemctl start blockdb
```

2. **Truncate Corrupted WAL** (last resort):
```bash
# Warning: This may cause data loss
sudo systemctl stop blockdb
sudo truncate -s 0 /var/lib/blockdb/wal.log
sudo systemctl start blockdb
```

## Monitoring and Diagnostics

### Health Check Script

Create `/usr/local/bin/blockdb-health.sh`:

```bash
#!/bin/bash

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "BlockDB Health Check"
echo "===================="

# Service status
if systemctl is-active --quiet blockdb; then
    echo -e "${GREEN}✓ Service is running${NC}"
else
    echo -e "${RED}✗ Service is not running${NC}"
    exit 1
fi

# Port availability
if nc -z localhost 8080; then
    echo -e "${GREEN}✓ Port 8080 is open${NC}"
else
    echo -e "${RED}✗ Port 8080 is not accessible${NC}"
fi

# Basic API test
if curl -s http://localhost:8080/health > /dev/null; then
    echo -e "${GREEN}✓ HTTP API is responding${NC}"
else
    echo -e "${RED}✗ HTTP API is not responding${NC}"
fi

# CLI test
if blockdb-cli stats > /dev/null 2>&1; then
    echo -e "${GREEN}✓ CLI is working${NC}"
else
    echo -e "${RED}✗ CLI is not working${NC}"
fi

# Blockchain integrity
if blockdb-cli verify | grep -q "verified successfully"; then
    echo -e "${GREEN}✓ Blockchain integrity OK${NC}"
else
    echo -e "${RED}✗ Blockchain integrity failed${NC}"
fi

# Disk space
USAGE=$(df /var/lib/blockdb | tail -1 | awk '{print $5}' | sed 's/%//')
if [ $USAGE -lt 80 ]; then
    echo -e "${GREEN}✓ Disk usage: ${USAGE}%${NC}"
elif [ $USAGE -lt 90 ]; then
    echo -e "${YELLOW}⚠ Disk usage: ${USAGE}%${NC}"
else
    echo -e "${RED}✗ Disk usage: ${USAGE}%${NC}"
fi

# Memory usage
MEM_USAGE=$(ps -o pid,vsz,rss,comm -p $(pgrep blockdb-server) | tail -1 | awk '{print $3}')
MEM_USAGE_MB=$((MEM_USAGE / 1024))
echo -e "${GREEN}✓ Memory usage: ${MEM_USAGE_MB}MB${NC}"

echo "===================="
echo "Health check completed"
```

### Performance Monitoring Script

Create `/usr/local/bin/blockdb-monitor.sh`:

```bash
#!/bin/bash

LOG_FILE="/var/log/blockdb-monitor.log"
INTERVAL=30

while true; do
    TIMESTAMP=$(date '+%Y-%m-%d %H:%M:%S')
    
    # Get process stats
    PID=$(pgrep blockdb-server)
    if [ -n "$PID" ]; then
        CPU=$(ps -o %cpu -p $PID | tail -1)
        MEM=$(ps -o %mem -p $PID | tail -1)
        RSS=$(ps -o rss -p $PID | tail -1)
        
        # Get I/O stats
        IO_READ=$(cat /proc/$PID/io | grep read_bytes | awk '{print $2}')
        IO_WRITE=$(cat /proc/$PID/io | grep write_bytes | awk '{print $2}')
        
        # Get network stats
        CONNECTIONS=$(ss -tn | grep :8080 | wc -l)
        
        echo "$TIMESTAMP,CPU:${CPU}%,MEM:${MEM}%,RSS:${RSS}KB,READ:${IO_READ},WRITE:${IO_WRITE},CONN:${CONNECTIONS}" >> $LOG_FILE
    else
        echo "$TIMESTAMP,ERROR: BlockDB process not found" >> $LOG_FILE
    fi
    
    sleep $INTERVAL
done
```

### Log Analysis

```bash
# Find error patterns
grep -i "error\|panic\|fail" /var/log/blockdb/blockdb.log | tail -20

# Analyze performance
grep -i "slow\|timeout\|latency" /var/log/blockdb/blockdb.log | tail -20

# Check consensus issues
grep -i "leader\|election\|consensus" /var/log/blockdb/blockdb.log | tail -20

# Monitor resource usage
grep -i "memory\|disk\|cpu" /var/log/blockdb/blockdb.log | tail -20
```

## Recovery Procedures

### Emergency Procedures

#### Complete Service Recovery

```bash
#!/bin/bash
# Emergency recovery script

echo "Starting BlockDB emergency recovery..."

# 1. Stop service
sudo systemctl stop blockdb

# 2. Backup current state
sudo cp -r /var/lib/blockdb /var/lib/blockdb.emergency.$(date +%Y%m%d_%H%M%S)

# 3. Restore from latest backup
if [ -d "/backup/blockdb/latest" ]; then
    sudo rsync -av /backup/blockdb/latest/ /var/lib/blockdb/
    echo "Restored from backup"
else
    echo "No backup found - manual intervention required"
    exit 1
fi

# 4. Fix permissions
sudo chown -R blockdb:blockdb /var/lib/blockdb

# 5. Start service
sudo systemctl start blockdb

# 6. Wait for startup
sleep 10

# 7. Verify health
if blockdb-cli stats > /dev/null 2>&1; then
    echo "Recovery successful"
    blockdb-cli verify
else
    echo "Recovery failed - check logs"
    journalctl -u blockdb -n 50
fi
```

#### Data Recovery from WAL

```bash
#!/bin/bash
# Recover data from WAL when main database is corrupted

echo "Starting WAL recovery..."

# Stop service
sudo systemctl stop blockdb

# Backup corrupted data
sudo mv /var/lib/blockdb /var/lib/blockdb.corrupted.$(date +%Y%m%d_%H%M%S)

# Create new data directory
sudo mkdir -p /var/lib/blockdb
sudo chown blockdb:blockdb /var/lib/blockdb

# Copy only WAL file
sudo cp /var/lib/blockdb.corrupted.*/wal.log /var/lib/blockdb/

# Start service (will replay WAL)
sudo systemctl start blockdb

# Wait and verify
sleep 15
blockdb-cli stats
blockdb-cli verify
```

### Cluster Recovery

#### Split-Brain Recovery

```bash
#!/bin/bash
# Recover from split-brain scenario

echo "Recovering from split-brain..."

# Stop all nodes
for node in node1 node2 node3; do
    ssh $node "sudo systemctl stop blockdb"
done

# Identify node with most recent data
for node in node1 node2 node3; do
    echo "=== $node ==="
    ssh $node "ls -la /var/lib/blockdb/ | grep wal"
done

# Choose authoritative node (usually the one with largest WAL)
AUTHORITY_NODE="node1"

# Copy data from authoritative node to others
for node in node2 node3; do
    ssh $node "sudo rm -rf /var/lib/blockdb/*"
    scp -r $AUTHORITY_NODE:/var/lib/blockdb/* $node:/var/lib/blockdb/
    ssh $node "sudo chown -R blockdb:blockdb /var/lib/blockdb"
done

# Start all nodes
for node in node1 node2 node3; do
    ssh $node "sudo systemctl start blockdb"
    sleep 5
done

# Verify cluster status
sleep 15
curl http://node1:8080/cluster/status
```

### Preventive Measures

#### Automated Backup

```bash
#!/bin/bash
# /usr/local/bin/blockdb-backup.sh

BACKUP_DIR="/backup/blockdb"
DATA_DIR="/var/lib/blockdb"
KEEP_DAYS=7

# Create timestamped backup
DATE=$(date +%Y%m%d_%H%M%S)
mkdir -p $BACKUP_DIR

# Hot backup (service running)
rsync -av --exclude='*.tmp' $DATA_DIR/ $BACKUP_DIR/backup_$DATE/

# Create "latest" symlink
ln -sfn $BACKUP_DIR/backup_$DATE $BACKUP_DIR/latest

# Cleanup old backups
find $BACKUP_DIR -name "backup_*" -type d -mtime +$KEEP_DAYS -exec rm -rf {} \;

# Verify backup
if [ -f "$BACKUP_DIR/latest/wal.log" ]; then
    echo "Backup successful: $DATE"
else
    echo "Backup failed: $DATE" | mail -s "BlockDB Backup Failed" admin@example.com
fi
```

#### Health Monitoring

```bash
#!/bin/bash
# /usr/local/bin/blockdb-watch.sh

while true; do
    if ! systemctl is-active --quiet blockdb; then
        echo "BlockDB service is down, attempting restart..."
        sudo systemctl start blockdb
        sleep 30
        
        if systemctl is-active --quiet blockdb; then
            echo "BlockDB restarted successfully"
        else
            echo "BlockDB restart failed" | mail -s "BlockDB Critical" admin@example.com
        fi
    fi
    
    sleep 60
done
```

This troubleshooting guide provides comprehensive coverage for diagnosing and resolving issues in BlockDB deployments. Regular use of the health check and monitoring scripts will help prevent many issues before they become critical.