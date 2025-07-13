# BlockDB Docker Guide

This guide covers Docker and Kubernetes deployment options for BlockDB, including single-node setups, multi-node clusters, and production deployments.

## Table of Contents

- [Quick Start](#quick-start)
- [Docker Setup](#docker-setup)
- [Docker Compose](#docker-compose)
- [Kubernetes Deployment](#kubernetes-deployment)
- [Configuration](#configuration)
- [Monitoring](#monitoring)
- [Troubleshooting](#troubleshooting)

## Quick Start

### Single Node with Docker

```bash
# Build the image
./scripts/docker-build.sh

# Run single node
docker-compose up -d

# Test the deployment
curl http://localhost:8080/health
```

### Multi-Node Cluster with Docker Compose

```bash
# Start 3-node cluster with load balancer
docker-compose -f docker-compose.cluster.yml up -d

# Test cluster health
curl http://localhost:8080/health
```

### Kubernetes Deployment

```bash
# Deploy to Kubernetes
./scripts/k8s-deploy.sh

# Check status
./scripts/k8s-deploy.sh --status

# Port forward for local access
./scripts/k8s-deploy.sh --port-forward
```

## Docker Setup

### Building Images

#### Quick Build
```bash
# Build with default settings
./scripts/docker-build.sh

# Build with custom tag
./scripts/docker-build.sh --tag v1.0.0

# Build for multiple platforms
./scripts/docker-build.sh --platform linux/amd64,linux/arm64 --push
```

#### Manual Build
```bash
# Standard build
docker build -t blockdb:latest .

# Multi-stage build optimization
docker build --target builder -t blockdb:builder .
docker build -t blockdb:latest .

# Build with build args
docker build --build-arg RUST_VERSION=1.75 -t blockdb:latest .
```

### Running Containers

#### Single Container
```bash
# Basic run
docker run -d \
  --name blockdb \
  -p 8080:8080 \
  -p 9090:9090 \
  -v blockdb_data:/var/lib/blockdb \
  blockdb:latest

# With custom configuration
docker run -d \
  --name blockdb \
  -p 8080:8080 \
  -e BLOCKDB_LOG_LEVEL=debug \
  -e BLOCKDB_MEMTABLE_SIZE=134217728 \
  -v ./my-config.toml:/etc/blockdb/blockdb.toml \
  -v blockdb_data:/var/lib/blockdb \
  blockdb:latest

# CLI access
docker run --rm -it \
  --network container:blockdb \
  blockdb:latest cli --help
```

#### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `BLOCKDB_NODE_ID` | Node identifier | `node1` |
| `BLOCKDB_HOST` | Server bind address | `0.0.0.0` |
| `BLOCKDB_PORT` | Server port | `8080` |
| `BLOCKDB_LOG_LEVEL` | Logging level | `info` |
| `BLOCKDB_DATA_DIR` | Data directory | `/var/lib/blockdb` |
| `BLOCKDB_PEERS` | Cluster peers (comma-separated) | - |
| `BLOCKDB_MEMTABLE_SIZE` | MemTable size in bytes | `67108864` |
| `BLOCKDB_HEARTBEAT_INTERVAL` | Raft heartbeat interval (ms) | `150` |
| `BLOCKDB_ELECTION_TIMEOUT` | Raft election timeout (ms) | `300` |

## Docker Compose

### Single Node Deployment

#### Basic Setup
```bash
# Start single node
docker-compose up -d

# View logs
docker-compose logs -f

# Stop and remove
docker-compose down
```

#### Custom Configuration
```yaml
# docker-compose.override.yml
version: '3.8'
services:
  blockdb:
    environment:
      - BLOCKDB_LOG_LEVEL=debug
      - BLOCKDB_MEMTABLE_SIZE=134217728
    volumes:
      - ./custom-config.toml:/etc/blockdb/blockdb.toml:ro
    ports:
      - "8080:8080"
      - "9090:9090"
```

### Multi-Node Cluster

#### 3-Node Cluster with Load Balancer
```bash
# Start cluster
docker-compose -f docker-compose.cluster.yml up -d

# Scale specific service
docker-compose -f docker-compose.cluster.yml up -d --scale blockdb-node2=2

# Check cluster status
curl http://localhost:8080/cluster/status
```

#### Load Balancer Configuration

The cluster setup includes an NGINX load balancer:

- **API Access**: `http://localhost:8080/api/`
- **Health Checks**: `http://localhost:8080/health`
- **Cluster Management**: `http://localhost:8080/cluster/`
- **Metrics**: `http://localhost:8080/metrics`

#### Node Access

Individual nodes are accessible on different ports:
- **Node 1**: `http://localhost:8081`
- **Node 2**: `http://localhost:8082`
- **Node 3**: `http://localhost:8083`

### Deployment Scripts

#### Automated Deployment
```bash
# Deploy single node
./scripts/docker-deploy.sh

# Deploy cluster
./scripts/docker-deploy.sh --type cluster

# Deploy with build
./scripts/docker-deploy.sh --type cluster --build

# Check status
./scripts/docker-deploy.sh --status

# View logs
./scripts/docker-deploy.sh --logs

# Stop deployment
./scripts/docker-deploy.sh --down
```

## Kubernetes Deployment

### Prerequisites

- Kubernetes cluster (1.20+)
- kubectl configured
- Sufficient resources (2 CPU, 4GB RAM per node)
- StorageClass for persistent volumes

### Quick Deployment

```bash
# Deploy with defaults
./scripts/k8s-deploy.sh

# Deploy to specific namespace
./scripts/k8s-deploy.sh --namespace blockdb-prod

# Deploy specific environment
./scripts/k8s-deploy.sh --environment production

# Dry run to preview changes
./scripts/k8s-deploy.sh --dry-run
```

### Manual Deployment

```bash
# Apply manifests
kubectl apply -f k8s/namespace.yaml
kubectl apply -f k8s/rbac.yaml
kubectl apply -f k8s/configmap.yaml
kubectl apply -f k8s/service.yaml
kubectl apply -f k8s/statefulset.yaml
kubectl apply -f k8s/pdb.yaml
kubectl apply -f k8s/ingress.yaml

# Wait for rollout
kubectl rollout status statefulset/blockdb -n blockdb

# Check status
kubectl get pods -n blockdb
```

### Scaling

```bash
# Scale to 5 nodes
kubectl scale statefulset blockdb --replicas=5 -n blockdb

# Check scaling progress
kubectl rollout status statefulset/blockdb -n blockdb
```

### Resource Requirements

#### Development
- **CPU**: 100m request, 500m limit
- **Memory**: 256Mi request, 1Gi limit
- **Storage**: 5Gi per node

#### Production
- **CPU**: 500m request, 2000m limit
- **Memory**: 1Gi request, 4Gi limit
- **Storage**: 50Gi+ per node

### Networking

#### Services
- **blockdb-headless**: StatefulSet service (internal)
- **blockdb**: Load balancer service
- **blockdb-external**: External access (optional)

#### Ingress
Configure `k8s/ingress.yaml` for external access:
```yaml
spec:
  rules:
  - host: blockdb.yourdomain.com
    http:
      paths:
      - path: /
        pathType: Prefix
        backend:
          service:
            name: blockdb
            port:
              number: 8080
```

### Storage

#### Storage Classes
```yaml
# Fast SSD storage class example
apiVersion: storage.k8s.io/v1
kind: StorageClass
metadata:
  name: fast-ssd
provisioner: kubernetes.io/aws-ebs
parameters:
  type: gp3
  iops: "3000"
  throughput: "125"
reclaimPolicy: Retain
```

Update StatefulSet to use specific storage class:
```yaml
volumeClaimTemplates:
- metadata:
    name: blockdb-data
  spec:
    accessModes: ["ReadWriteOnce"]
    storageClassName: fast-ssd
    resources:
      requests:
        storage: 50Gi
```

## Configuration

### Docker Configuration

#### Custom Config File
```toml
# custom-blockdb.toml
[database]
data_dir = "/var/lib/blockdb"
memtable_size_limit = 268435456  # 256MB
wal_sync_interval = 500
compaction_threshold = 8
blockchain_batch_size = 5000

[server]
host = "0.0.0.0"
port = 8080
worker_threads = 8

[cluster]
node_id = "custom-node"
heartbeat_interval = 100
election_timeout = 200
enable_transactions = true
transaction_timeout = 60

[logging]
level = "debug"
file = "/var/log/blockdb/blockdb.log"
```

Mount as volume:
```bash
docker run -d \
  -v ./custom-blockdb.toml:/etc/blockdb/blockdb.toml:ro \
  blockdb:latest
```

### Kubernetes Configuration

#### ConfigMap Updates
```bash
# Update ConfigMap
kubectl create configmap blockdb-config \
  --from-file=blockdb.toml=./custom-config.toml \
  --namespace=blockdb \
  --dry-run=client -o yaml | kubectl apply -f -

# Restart StatefulSet to pick up changes
kubectl rollout restart statefulset/blockdb -n blockdb
```

#### Secrets Management
```bash
# Create secret for sensitive data
kubectl create secret generic blockdb-secrets \
  --from-literal=database-password=secretpass \
  --from-literal=jwt-secret=jwtsecret \
  --namespace=blockdb
```

## Monitoring

### Health Checks

#### Docker
```bash
# Check container health
docker ps --filter name=blockdb

# Health check endpoint
curl http://localhost:8080/health

# Container logs
docker logs blockdb -f
```

#### Kubernetes
```bash
# Pod status
kubectl get pods -n blockdb

# Health check via service
curl http://localhost:8080/health

# Pod logs
kubectl logs -f statefulset/blockdb -n blockdb
```

### Metrics

#### Prometheus Metrics
BlockDB exposes Prometheus metrics on port 9090:

```bash
# Docker
curl http://localhost:9090/metrics

# Kubernetes (with port-forward)
kubectl port-forward service/blockdb 9090:9090 -n blockdb
curl http://localhost:9090/metrics
```

#### Key Metrics
- `blockdb_operations_total` - Total operations
- `blockdb_operation_duration_seconds` - Operation latency
- `blockdb_memory_usage_bytes` - Memory usage
- `blockdb_storage_size_bytes` - Storage usage
- `blockdb_cluster_nodes` - Cluster node count
- `blockdb_raft_term` - Current Raft term

### Log Aggregation

#### Docker Logging
```bash
# Configure log driver
docker run -d \
  --log-driver=fluentd \
  --log-opt fluentd-address=localhost:24224 \
  --log-opt tag="blockdb.{{.Name}}" \
  blockdb:latest
```

#### Kubernetes Logging
```yaml
# FluentBit DaemonSet for log collection
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: fluent-bit
spec:
  template:
    spec:
      containers:
      - name: fluent-bit
        image: fluent/fluent-bit:latest
        volumeMounts:
        - name: varlog
          mountPath: /var/log
        - name: varlibdockercontainers
          mountPath: /var/lib/docker/containers
          readOnly: true
```

## Troubleshooting

### Common Issues

#### Container Won't Start
```bash
# Check logs
docker logs blockdb

# Common issues:
# 1. Port conflicts
# 2. Permission issues
# 3. Invalid configuration

# Fix permissions
docker run --rm -v blockdb_data:/data alpine chmod -R 755 /data
```

#### Cluster Formation Issues
```bash
# Check individual nodes
curl http://localhost:8081/health
curl http://localhost:8082/health
curl http://localhost:8083/health

# Check cluster status
curl http://localhost:8081/cluster/status

# Restart problematic node
docker-compose -f docker-compose.cluster.yml restart blockdb-node2
```

#### Kubernetes Issues
```bash
# Pod stuck in pending
kubectl describe pod blockdb-0 -n blockdb

# PVC issues
kubectl get pvc -n blockdb
kubectl describe pvc blockdb-data-blockdb-0 -n blockdb

# Service discovery issues
kubectl get endpoints -n blockdb
```

### Debugging Tools

#### Container Shell Access
```bash
# Docker
docker exec -it blockdb bash

# Kubernetes
kubectl exec -it blockdb-0 -n blockdb -- bash
```

#### Network Debugging
```bash
# Test connectivity between containers
docker run --rm --network blockdb_blockdb-cluster alpine ping blockdb-node1

# Check port accessibility
docker run --rm --network blockdb_blockdb-cluster alpine nc -zv blockdb-node1 8080
```

#### Resource Monitoring
```bash
# Docker resource usage
docker stats blockdb

# Kubernetes resource usage
kubectl top pods -n blockdb
kubectl describe pod blockdb-0 -n blockdb
```

### Performance Tuning

#### Docker Performance
```bash
# Increase memory limits
docker run -d \
  --memory=4g \
  --memory-swap=4g \
  --cpus=2 \
  blockdb:latest

# Use host networking for performance
docker run -d \
  --network=host \
  blockdb:latest
```

#### Kubernetes Performance
```yaml
# Resource requests and limits
resources:
  requests:
    memory: "2Gi"
    cpu: "1000m"
  limits:
    memory: "4Gi"
    cpu: "2000m"

# Node affinity for dedicated nodes
affinity:
  nodeAffinity:
    requiredDuringSchedulingIgnoredDuringExecution:
      nodeSelectorTerms:
      - matchExpressions:
        - key: node-type
          operator: In
          values:
          - database
```

#### Storage Performance
```yaml
# Use faster storage classes
storageClassName: fast-ssd

# Increase storage size
resources:
  requests:
    storage: 100Gi
```

## Production Checklist

### Security
- [ ] Use non-root user in containers
- [ ] Enable TLS/SSL for external access
- [ ] Implement network policies
- [ ] Regular security updates
- [ ] Secret management for sensitive data

### High Availability
- [ ] Multiple nodes (minimum 3)
- [ ] PodDisruptionBudget configured
- [ ] Node anti-affinity rules
- [ ] Health checks configured
- [ ] Load balancer setup

### Performance
- [ ] Resource limits configured
- [ ] Storage class optimized
- [ ] Network optimization
- [ ] Monitoring and alerting
- [ ] Log aggregation

### Backup & Recovery
- [ ] Persistent volume snapshots
- [ ] Data backup strategy
- [ ] Disaster recovery plan
- [ ] Regular backup testing
- [ ] Cross-region replication

### Monitoring
- [ ] Prometheus metrics collection
- [ ] Grafana dashboards
- [ ] Alert rules configured
- [ ] Log aggregation
- [ ] Performance monitoring

This comprehensive Docker guide provides everything needed to deploy and operate BlockDB in containerized environments, from development to production.