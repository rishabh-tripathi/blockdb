# BlockDB Deployment Guide

## Overview

This guide covers deploying BlockDB in various environments, from single-node development setups to production-ready distributed clusters.

## Table of Contents

- [Single Node Deployment](#single-node-deployment)
- [Multi-Node Cluster Deployment](#multi-node-cluster-deployment)
- [Container Deployment](#container-deployment)
- [Cloud Deployment](#cloud-deployment)
- [Production Considerations](#production-considerations)
- [Monitoring Setup](#monitoring-setup)
- [Backup and Recovery](#backup-and-recovery)

## Single Node Deployment

### Prerequisites

**System Requirements:**
- **OS**: Linux (Ubuntu 20.04+), macOS (10.15+), or Windows 10+
- **CPU**: 2+ cores recommended
- **Memory**: 4GB+ RAM recommended
- **Storage**: 10GB+ available space (SSD preferred)
- **Network**: Stable internet connection

**Dependencies:**
- Rust 1.70.0 or later
- Git (for source installation)

### Installation Methods

#### Method 1: Binary Installation

```bash
# Download latest release
wget https://github.com/username/blockdb/releases/latest/download/blockdb-linux-x64.tar.gz

# Extract binaries
tar -xzf blockdb-linux-x64.tar.gz

# Install globally (optional)
sudo mv blockdb-cli blockdb-server /usr/local/bin/

# Verify installation
blockdb-cli --version
blockdb-server --help
```

#### Method 2: Source Installation

```bash
# Clone repository
git clone https://github.com/username/blockdb.git
cd blockdb

# Build release binaries
cargo build --release

# Binaries available at:
# ./target/release/blockdb-cli
# ./target/release/blockdb-server
```

### Configuration

Create a configuration file at `/etc/blockdb/blockdb.toml`:

```toml
[database]
data_dir = "/var/lib/blockdb"
memtable_size_limit = 67108864    # 64MB
wal_sync_interval = 1000          # 1 second
compaction_threshold = 4
blockchain_batch_size = 1000

[server]
host = "0.0.0.0"
port = 8080
max_connections = 1000
request_timeout = 30
enable_cors = true
enable_compression = true

[logging]
level = "info"
file = "/var/log/blockdb/blockdb.log"
```

### System Service Setup

#### SystemD Service (Linux)

Create `/etc/systemd/system/blockdb.service`:

```ini
[Unit]
Description=BlockDB Server
After=network.target
Wants=network.target

[Service]
Type=simple
User=blockdb
Group=blockdb
ExecStart=/usr/local/bin/blockdb-server --config /etc/blockdb/blockdb.toml
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal
SyslogIdentifier=blockdb

# Security settings
NoNewPrivileges=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectHome=yes
ProtectSystem=strict
ReadWritePaths=/var/lib/blockdb /var/log/blockdb

[Install]
WantedBy=multi-user.target
```

Setup and start the service:

```bash
# Create user and directories
sudo useradd -r -s /bin/false blockdb
sudo mkdir -p /var/lib/blockdb /var/log/blockdb /etc/blockdb
sudo chown blockdb:blockdb /var/lib/blockdb /var/log/blockdb
sudo chmod 755 /var/lib/blockdb /var/log/blockdb

# Enable and start service
sudo systemctl daemon-reload
sudo systemctl enable blockdb
sudo systemctl start blockdb

# Check status
sudo systemctl status blockdb
sudo journalctl -u blockdb -f
```

#### Launchd Service (macOS)

Create `~/Library/LaunchAgents/com.blockdb.server.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.blockdb.server</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/blockdb-server</string>
        <string>--config</string>
        <string>/usr/local/etc/blockdb.toml</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/usr/local/var/log/blockdb.log</string>
    <key>StandardErrorPath</key>
    <string>/usr/local/var/log/blockdb.log</string>
</dict>
</plist>
```

Load and start:

```bash
launchctl load ~/Library/LaunchAgents/com.blockdb.server.plist
launchctl start com.blockdb.server
```

### Basic Testing

```bash
# Test CLI operations
blockdb-cli put "test:key" "test value"
blockdb-cli get "test:key"
blockdb-cli verify
blockdb-cli stats

# Test HTTP API (if server is running)
curl -X POST http://localhost:8080/api/v1/put \
  -H "Content-Type: application/json" \
  -d '{"key": "api:test", "value": "API test value"}'

curl http://localhost:8080/api/v1/get/api:test
```

## Multi-Node Cluster Deployment

### Cluster Planning

**Recommended Configurations:**

| Cluster Size | Fault Tolerance | Use Case |
|--------------|-----------------|----------|
| 3 nodes | 1 node failure | Development/Testing |
| 5 nodes | 2 node failures | Production |
| 7 nodes | 3 node failures | High availability |

**Network Requirements:**
- **Latency**: < 10ms between nodes
- **Bandwidth**: 100Mbps+ for replication
- **Ports**: 8080 (configurable) open between nodes

### Node Configuration

#### Node 1 (Leader candidate)

Configuration file `/etc/blockdb/node1.toml`:

```toml
[database]
data_dir = "/var/lib/blockdb/node1"
memtable_size_limit = 134217728   # 128MB for production
wal_sync_interval = 500           # Faster sync for consistency
compaction_threshold = 6
blockchain_batch_size = 2000

[server]
host = "192.168.1.10"
port = 8080
max_connections = 2000
request_timeout = 30

[cluster]
node_id = "node1"
address = "192.168.1.10:8080"
peers = [
    "node2:192.168.1.11:8080",
    "node3:192.168.1.12:8080"
]
heartbeat_interval = 100          # 100ms for fast failure detection
election_timeout = 200            # 200ms election timeout
enable_transactions = true
transaction_timeout = 30

[logging]
level = "info"
file = "/var/log/blockdb/node1.log"
```

#### Node 2 (Follower)

Configuration file `/etc/blockdb/node2.toml`:

```toml
[database]
data_dir = "/var/lib/blockdb/node2"
memtable_size_limit = 134217728
wal_sync_interval = 500
compaction_threshold = 6
blockchain_batch_size = 2000

[server]
host = "192.168.1.11"
port = 8080
max_connections = 2000
request_timeout = 30

[cluster]
node_id = "node2"
address = "192.168.1.11:8080"
peers = [
    "node1:192.168.1.10:8080",
    "node3:192.168.1.12:8080"
]
heartbeat_interval = 100
election_timeout = 200
enable_transactions = true
transaction_timeout = 30

[logging]
level = "info"
file = "/var/log/blockdb/node2.log"
```

#### Node 3 (Follower)

Configuration file `/etc/blockdb/node3.toml`:

```toml
[database]
data_dir = "/var/lib/blockdb/node3"
memtable_size_limit = 134217728
wal_sync_interval = 500
compaction_threshold = 6
blockchain_batch_size = 2000

[server]
host = "192.168.1.12"
port = 8080
max_connections = 2000
request_timeout = 30

[cluster]
node_id = "node3"
address = "192.168.1.12:8080"
peers = [
    "node1:192.168.1.10:8080",
    "node2:192.168.1.11:8080"
]
heartbeat_interval = 100
election_timeout = 200
enable_transactions = true
transaction_timeout = 30

[logging]
level = "info"
file = "/var/log/blockdb/node3.log"
```

### Cluster Startup Sequence

```bash
# Start all nodes simultaneously
# Node 1
ssh node1 "sudo systemctl start blockdb"

# Node 2  
ssh node2 "sudo systemctl start blockdb"

# Node 3
ssh node3 "sudo systemctl start blockdb"

# Wait for leader election (5-10 seconds)
sleep 10

# Verify cluster status
curl http://192.168.1.10:8080/cluster/status
curl http://192.168.1.11:8080/cluster/status
curl http://192.168.1.12:8080/cluster/status
```

### Load Balancer Configuration

#### HAProxy Configuration

Create `/etc/haproxy/haproxy.cfg`:

```
global
    daemon
    maxconn 4096

defaults
    mode http
    timeout connect 5000ms
    timeout client 50000ms
    timeout server 50000ms

frontend blockdb_frontend
    bind *:80
    default_backend blockdb_cluster

backend blockdb_cluster
    balance roundrobin
    option httpchk GET /health
    
    server node1 192.168.1.10:8080 check
    server node2 192.168.1.11:8080 check
    server node3 192.168.1.12:8080 check
```

#### NGINX Configuration

Create `/etc/nginx/conf.d/blockdb.conf`:

```nginx
upstream blockdb_cluster {
    server 192.168.1.10:8080;
    server 192.168.1.11:8080;
    server 192.168.1.12:8080;
}

server {
    listen 80;
    server_name blockdb.example.com;

    location / {
        proxy_pass http://blockdb_cluster;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_connect_timeout 30s;
        proxy_send_timeout 30s;
        proxy_read_timeout 30s;
    }

    location /health {
        proxy_pass http://blockdb_cluster/health;
        access_log off;
    }
}
```

## Container Deployment

### Docker Configuration

#### Dockerfile

```dockerfile
FROM rust:1.70 as builder

WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -r -s /bin/false blockdb

COPY --from=builder /app/target/release/blockdb-server /usr/local/bin/
COPY --from=builder /app/target/release/blockdb-cli /usr/local/bin/

USER blockdb
EXPOSE 8080

VOLUME ["/data"]

CMD ["blockdb-server", "--data-dir", "/data", "--host", "0.0.0.0", "--port", "8080"]
```

#### Docker Compose

Create `docker-compose.yml`:

```yaml
version: '3.8'

services:
  blockdb-node1:
    build: .
    container_name: blockdb-node1
    ports:
      - "8081:8080"
    volumes:
      - blockdb-node1-data:/data
      - ./config/node1.toml:/etc/blockdb/blockdb.toml:ro
    environment:
      - RUST_LOG=info
    networks:
      - blockdb-cluster

  blockdb-node2:
    build: .
    container_name: blockdb-node2
    ports:
      - "8082:8080"
    volumes:
      - blockdb-node2-data:/data
      - ./config/node2.toml:/etc/blockdb/blockdb.toml:ro
    environment:
      - RUST_LOG=info
    networks:
      - blockdb-cluster

  blockdb-node3:
    build: .
    container_name: blockdb-node3
    ports:
      - "8083:8080"
    volumes:
      - blockdb-node3-data:/data
      - ./config/node3.toml:/etc/blockdb/blockdb.toml:ro
    environment:
      - RUST_LOG=info
    networks:
      - blockdb-cluster

  nginx:
    image: nginx:alpine
    container_name: blockdb-nginx
    ports:
      - "80:80"
    volumes:
      - ./nginx.conf:/etc/nginx/nginx.conf:ro
    depends_on:
      - blockdb-node1
      - blockdb-node2
      - blockdb-node3
    networks:
      - blockdb-cluster

volumes:
  blockdb-node1-data:
  blockdb-node2-data:
  blockdb-node3-data:

networks:
  blockdb-cluster:
    driver: bridge
```

#### Deploy with Docker Compose

```bash
# Build and start cluster
docker-compose up -d

# Check logs
docker-compose logs -f

# Test deployment
curl http://localhost/health

# Scale cluster (add more nodes)
docker-compose up -d --scale blockdb-node=5
```

### Kubernetes Deployment

#### ConfigMap

Create `k8s/configmap.yaml`:

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: blockdb-config
data:
  blockdb.toml: |
    [database]
    data_dir = "/data"
    memtable_size_limit = 134217728
    wal_sync_interval = 500
    compaction_threshold = 6
    blockchain_batch_size = 2000

    [server]
    host = "0.0.0.0"
    port = 8080
    max_connections = 2000
    request_timeout = 30

    [cluster]
    enable_transactions = true
    transaction_timeout = 30
    heartbeat_interval = 100
    election_timeout = 200

    [logging]
    level = "info"
```

#### StatefulSet

Create `k8s/statefulset.yaml`:

```yaml
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: blockdb
spec:
  serviceName: blockdb-headless
  replicas: 3
  selector:
    matchLabels:
      app: blockdb
  template:
    metadata:
      labels:
        app: blockdb
    spec:
      containers:
      - name: blockdb
        image: blockdb:latest
        ports:
        - containerPort: 8080
          name: http
        env:
        - name: POD_NAME
          valueFrom:
            fieldRef:
              fieldPath: metadata.name
        - name: POD_NAMESPACE
          valueFrom:
            fieldRef:
              fieldPath: metadata.namespace
        volumeMounts:
        - name: data
          mountPath: /data
        - name: config
          mountPath: /etc/blockdb
        resources:
          requests:
            memory: "1Gi"
            cpu: "500m"
          limits:
            memory: "2Gi"
            cpu: "1000m"
        livenessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 30
          periodSeconds: 10
        readinessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 5
          periodSeconds: 5
      volumes:
      - name: config
        configMap:
          name: blockdb-config
  volumeClaimTemplates:
  - metadata:
      name: data
    spec:
      accessModes: ["ReadWriteOnce"]
      resources:
        requests:
          storage: 10Gi
```

#### Services

Create `k8s/services.yaml`:

```yaml
apiVersion: v1
kind: Service
metadata:
  name: blockdb-headless
spec:
  clusterIP: None
  selector:
    app: blockdb
  ports:
  - port: 8080
    targetPort: 8080
    name: http

---
apiVersion: v1
kind: Service
metadata:
  name: blockdb-service
spec:
  selector:
    app: blockdb
  ports:
  - port: 8080
    targetPort: 8080
    name: http
  type: LoadBalancer
```

#### Deploy to Kubernetes

```bash
# Apply configurations
kubectl apply -f k8s/configmap.yaml
kubectl apply -f k8s/statefulset.yaml
kubectl apply -f k8s/services.yaml

# Check deployment
kubectl get pods -l app=blockdb
kubectl get svc blockdb-service

# Test deployment
kubectl port-forward svc/blockdb-service 8080:8080
curl http://localhost:8080/health
```

## Cloud Deployment

### AWS Deployment

#### EC2 Instance Setup

```bash
# Launch EC2 instances (t3.medium or larger)
aws ec2 run-instances \
  --image-id ami-0abcdef1234567890 \
  --count 3 \
  --instance-type t3.medium \
  --key-name my-key-pair \
  --security-group-ids sg-12345678 \
  --subnet-id subnet-12345678 \
  --user-data file://user-data.sh
```

User data script (`user-data.sh`):

```bash
#!/bin/bash
apt-get update
apt-get install -y curl wget

# Install BlockDB
wget https://github.com/username/blockdb/releases/latest/download/blockdb-linux-x64.tar.gz
tar -xzf blockdb-linux-x64.tar.gz
mv blockdb-* /usr/local/bin/

# Create user and directories
useradd -r -s /bin/false blockdb
mkdir -p /var/lib/blockdb /var/log/blockdb /etc/blockdb
chown blockdb:blockdb /var/lib/blockdb /var/log/blockdb

# Install systemd service
cat > /etc/systemd/system/blockdb.service << 'EOF'
[Unit]
Description=BlockDB Server
After=network.target

[Service]
Type=simple
User=blockdb
Group=blockdb
ExecStart=/usr/local/bin/blockdb-server --config /etc/blockdb/blockdb.toml
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable blockdb
```

#### ELB Configuration

```bash
# Create Application Load Balancer
aws elbv2 create-load-balancer \
  --name blockdb-alb \
  --subnets subnet-12345678 subnet-87654321 \
  --security-groups sg-12345678

# Create target group
aws elbv2 create-target-group \
  --name blockdb-targets \
  --protocol HTTP \
  --port 8080 \
  --vpc-id vpc-12345678 \
  --health-check-path /health

# Register targets
aws elbv2 register-targets \
  --target-group-arn arn:aws:elasticloadbalancing:... \
  --targets Id=i-1234567890abcdef0 Id=i-0987654321fedcba0
```

### Google Cloud Platform

#### GKE Deployment

```bash
# Create GKE cluster
gcloud container clusters create blockdb-cluster \
  --num-nodes=3 \
  --machine-type=n1-standard-2 \
  --zone=us-central1-a

# Deploy BlockDB
kubectl apply -f k8s/

# Create load balancer
kubectl expose deployment blockdb --type=LoadBalancer --port=80 --target-port=8080
```

### Azure Deployment

#### AKS Deployment

```bash
# Create resource group
az group create --name blockdb-rg --location eastus

# Create AKS cluster
az aks create \
  --resource-group blockdb-rg \
  --name blockdb-aks \
  --node-count 3 \
  --node-vm-size Standard_D2s_v3 \
  --generate-ssh-keys

# Deploy BlockDB
kubectl apply -f k8s/
```

## Production Considerations

### Hardware Requirements

**Production Recommendations:**

| Component | Minimum | Recommended | High Performance |
|-----------|---------|-------------|------------------|
| CPU | 2 cores | 4 cores | 8+ cores |
| RAM | 8GB | 16GB | 32GB+ |
| Storage | 100GB SSD | 500GB NVMe | 1TB+ NVMe |
| Network | 1Gbps | 10Gbps | 25Gbps+ |

### Security Hardening

#### Operating System

```bash
# Update system
apt-get update && apt-get upgrade -y

# Configure firewall
ufw allow 22/tcp    # SSH
ufw allow 8080/tcp  # BlockDB
ufw enable

# Disable unnecessary services
systemctl disable bluetooth
systemctl disable cups
systemctl disable avahi-daemon

# Configure fail2ban
apt-get install fail2ban
systemctl enable fail2ban
```

#### Application Security

```toml
# Security configuration
[security]
enable_tls = true
tls_cert_file = "/etc/ssl/certs/blockdb.crt"
tls_key_file = "/etc/ssl/private/blockdb.key"
enable_auth = true
auth_token_file = "/etc/blockdb/tokens.json"
max_request_size = 1048576  # 1MB
rate_limit_requests_per_second = 100
```

### Performance Tuning

#### System-Level Tuning

```bash
# Increase file descriptor limits
echo "blockdb soft nofile 65536" >> /etc/security/limits.conf
echo "blockdb hard nofile 65536" >> /etc/security/limits.conf

# Optimize kernel parameters
cat >> /etc/sysctl.conf << EOF
# Network optimizations
net.core.rmem_max = 16777216
net.core.wmem_max = 16777216
net.ipv4.tcp_rmem = 4096 12582912 16777216
net.ipv4.tcp_wmem = 4096 12582912 16777216

# File system optimizations
vm.dirty_ratio = 15
vm.dirty_background_ratio = 5
vm.swappiness = 1
EOF

sysctl -p
```

#### Database Tuning

```toml
# High-performance configuration
[database]
memtable_size_limit = 268435456    # 256MB
wal_sync_interval = 100            # 100ms for low latency
compaction_threshold = 8           # Less frequent compaction
blockchain_batch_size = 5000       # Larger batches

[server]
max_connections = 5000
request_timeout = 60
enable_compression = true
worker_threads = 8                 # Match CPU cores
```

### Monitoring Setup

#### Prometheus Configuration

Create `prometheus.yml`:

```yaml
global:
  scrape_interval: 15s

scrape_configs:
  - job_name: 'blockdb'
    static_configs:
      - targets: ['localhost:8080']
    metrics_path: /metrics
    scrape_interval: 5s
```

#### Grafana Dashboard

Import the BlockDB dashboard or create custom panels:

```json
{
  "dashboard": {
    "title": "BlockDB Metrics",
    "panels": [
      {
        "title": "Write Throughput",
        "type": "graph",
        "targets": [
          {
            "expr": "rate(blockdb_writes_total[5m])",
            "legendFormat": "Writes/sec"
          }
        ]
      },
      {
        "title": "Read Latency",
        "type": "graph",
        "targets": [
          {
            "expr": "histogram_quantile(0.95, rate(blockdb_read_duration_seconds_bucket[5m]))",
            "legendFormat": "95th percentile"
          }
        ]
      }
    ]
  }
}
```

### Backup and Recovery

#### Automated Backup Script

Create `/usr/local/bin/blockdb-backup.sh`:

```bash
#!/bin/bash

BACKUP_DIR="/backup/blockdb"
DATA_DIR="/var/lib/blockdb"
DATE=$(date +%Y%m%d_%H%M%S)

# Create backup directory
mkdir -p $BACKUP_DIR

# Stop BlockDB service
systemctl stop blockdb

# Create backup
tar -czf $BACKUP_DIR/blockdb_backup_$DATE.tar.gz -C $DATA_DIR .

# Restart BlockDB service
systemctl start blockdb

# Keep only last 7 days of backups
find $BACKUP_DIR -name "blockdb_backup_*.tar.gz" -mtime +7 -delete

echo "Backup completed: blockdb_backup_$DATE.tar.gz"
```

#### Cron Job

```bash
# Add to crontab
0 2 * * * /usr/local/bin/blockdb-backup.sh >> /var/log/blockdb-backup.log 2>&1
```

#### Recovery Procedure

```bash
# Stop BlockDB
systemctl stop blockdb

# Restore data
cd /var/lib/blockdb
tar -xzf /backup/blockdb/blockdb_backup_YYYYMMDD_HHMMSS.tar.gz

# Fix permissions
chown -R blockdb:blockdb /var/lib/blockdb

# Start BlockDB
systemctl start blockdb

# Verify integrity
blockdb-cli verify
```

### Disaster Recovery

#### Multi-Region Setup

1. **Primary Region**: Full cluster (3+ nodes)
2. **Secondary Region**: Standby cluster with replication
3. **Cross-Region Replication**: Async replication for DR

#### Failover Procedure

```bash
# Promote secondary region to primary
# 1. Stop replication
# 2. Update DNS/load balancer
# 3. Redirect traffic to secondary region
# 4. Monitor and validate
```

This deployment guide provides comprehensive coverage for deploying BlockDB in various environments, from development to production-scale deployments.