# 🚀 BlockDB Usage Demo

This document demonstrates how to use BlockDB, a high-performance distributed append-only database with blockchain verification and authentication.

## 📦 Basic Setup

```rust
use blockdb::{BlockDBHandle, BlockDBConfig};
use tempfile::TempDir;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a temporary directory for database files
    let temp_dir = TempDir::new()?;
    
    // Configure the database
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        memtable_size_limit: 64 * 1024 * 1024, // 64MB
        wal_sync_interval_ms: 1000,
        compaction_threshold: 4,
        blockchain_batch_size: 1000,
    };

    // Initialize the database
    let db = BlockDBHandle::new(config)?;
    
    println!("✅ BlockDB initialized successfully!");
    Ok(())
}
```

## 📝 Basic Operations

### 1. **Put Operation (Write Data)**
```rust
// Store key-value pairs
db.put(b"user:1001", b"{'name': 'Alice', 'age': 30}").await?;
db.put(b"user:1002", b"{'name': 'Bob', 'age': 25}").await?;
db.put(b"config:theme", b"dark").await?;

println!("✅ Data stored successfully!");
```

### 2. **Get Operation (Read Data)**
```rust
// Retrieve data by key
if let Some(user_data) = db.get(b"user:1001").await? {
    let user_str = String::from_utf8(user_data)?;
    println!("📖 User 1001: {}", user_str);
}

// Handle missing keys
match db.get(b"user:9999").await? {
    Some(data) => println!("Found: {:?}", data),
    None => println!("❌ User 9999 not found"),
}
```

### 3. **Append-Only Behavior**
```rust
// First write succeeds
db.put(b"counter", b"1").await?;
println!("✅ Initial counter set to 1");

// Attempting to update the same key fails (append-only)
match db.put(b"counter", b"2").await {
    Ok(_) => println!("This should not happen!"),
    Err(e) => println!("❌ Expected error: {}", e), // DuplicateKey error
}

// Original value is preserved
let counter = db.get(b"counter").await?.unwrap();
assert_eq!(counter, b"1");
println!("✅ Original value preserved: {}", String::from_utf8(counter)?);
```

## 🔗 Blockchain Verification

### **Verify Data Integrity**
```rust
// Verify the blockchain integrity
let is_valid = db.verify_integrity().await?;
if is_valid {
    println!("✅ Blockchain verification passed - all data is authentic!");
} else {
    println!("❌ Blockchain verification failed - data may be corrupted!");
}
```

### **View Blockchain Metrics**
```rust
// Get blockchain statistics
let stats = db.get_blockchain_stats().await?;
println!("📊 Blockchain Stats:");
println!("   - Total blocks: {}", stats.total_blocks);
println!("   - Total operations: {}", stats.total_operations);
println!("   - Last block hash: {}", stats.last_block_hash);
```

## 🌐 Distributed Setup (Multi-Node)

### **Node Configuration**
```rust
use blockdb::{DistributedBlockDB, DistributedBlockDBConfig, ClusterConfig};
use std::collections::HashMap;

// Configure cluster nodes
let mut nodes = HashMap::new();
nodes.insert("node1".to_string(), NodeAddress { host: "127.0.0.1".to_string(), port: 8080 });
nodes.insert("node2".to_string(), NodeAddress { host: "127.0.0.1".to_string(), port: 8081 });
nodes.insert("node3".to_string(), NodeAddress { host: "127.0.0.1".to_string(), port: 8082 });

let config = DistributedBlockDBConfig {
    node_id: "node1".to_string(),
    data_dir: "./node1_data".to_string(),
    cluster: ClusterConfig {
        nodes,
        heartbeat_interval_ms: 150,
        election_timeout_ms: 300,
        enable_transactions: true,
        transaction_timeout_secs: 30,
    },
    memtable_size_limit: 64 * 1024 * 1024,
    wal_sync_interval_ms: 1000,
    compaction_threshold: 4,
    blockchain_batch_size: 1000,
};

// Start distributed node
let db = DistributedBlockDB::new(config).await?;
db.start().await?;

println!("🌐 Distributed node started!");
```

### **Consensus Operations**
```rust
// Operations go through Raft consensus
db.put(b"distributed_key", b"replicated_value").await?;
println!("✅ Data replicated across all nodes!");

// Read from any node
let value = db.get(b"distributed_key").await?;
println!("📖 Retrieved from cluster: {:?}", value);
```

## 🔐 Authentication Features (When Available)

### **User Management**
```rust
use blockdb::{AuthenticatedDistributedBlockDB, AuthConfig, PermissionSet, Permission};

// Configure authentication
let auth_config = AuthConfig {
    enabled: true,
    session_duration_hours: 24,
    max_failed_attempts: 3,
    password_min_length: 8,
    require_strong_passwords: true,
    admin_users: vec!["admin".to_string()],
    allow_anonymous_reads: false,
    token_refresh_threshold_hours: 4,
};

let db = AuthenticatedDistributedBlockDB::new(config_with_auth).await?;

// Authenticate admin user
let admin_context = db.authenticate("admin", "admin_password").await?;
println!("🔐 Admin authenticated successfully!");

// Create a new user
let user_permissions = PermissionSet::read_write();
db.create_user("alice", "AlicePassword123!", user_permissions, &admin_context).await?;
println!("👤 User 'alice' created!");
```

### **Authenticated Operations**
```rust
// Authenticate regular user
let alice_context = db.authenticate("alice", "AlicePassword123!").await?;

// Perform authenticated operations
db.authenticated_put(b"alice_data", b"private_info", &alice_context).await?;
let data = db.authenticated_get(b"alice_data", &alice_context).await?;

println!("🔒 Authenticated operation completed!");
```

### **Permission Management**
```rust
// Grant additional permissions
db.grant_permission("alice", Permission::Delete, &admin_context).await?;
println!("✅ Delete permission granted to alice");

// Revoke permissions
db.revoke_permission("alice", &Permission::Delete, &admin_context).await?;
println!("❌ Delete permission revoked from alice");

// View audit trail
let audit_trail = db.get_auth_audit_trail().await?;
println!("📋 Audit trail has {} entries", audit_trail.len());
```

## 🔄 Transaction Support

### **ACID Transactions**
```rust
// Execute a transaction
let result = db.execute_transaction(|tx| async move {
    // All operations in this block are atomic
    tx.put(b"account:alice", b"balance:1000").await?;
    tx.put(b"account:bob", b"balance:500").await?;
    
    // Transfer money (all or nothing)
    let alice_balance = tx.get(b"account:alice").await?;
    let bob_balance = tx.get(b"account:bob").await?;
    
    // Business logic here...
    println!("💰 Transfer completed in transaction");
    
    Ok(())
}).await?;

println!("✅ Transaction committed successfully!");
```

### **Authenticated Transactions**
```rust
// Transaction with user authentication
db.execute_authenticated_transaction(&alice_context, |tx| async move {
    tx.put(b"alice:private", b"secret_data").await?;
    tx.put(b"alice:config", b"theme:dark").await?;
    
    Ok(())
}).await?;

println!("🔐 Authenticated transaction completed!");
```

## 📊 Monitoring and Statistics

### **Database Metrics**
```rust
// Get performance statistics
let stats = db.get_stats().await?;
println!("📈 Database Statistics:");
println!("   - Total operations: {}", stats.total_operations);
println!("   - Cache hit rate: {:.2}%", stats.cache_hit_rate * 100.0);
println!("   - Average latency: {}ms", stats.average_latency_ms);
println!("   - Memory usage: {}MB", stats.memory_usage_mb);
```

### **Health Checks**
```rust
// Check database health
let health = db.health_check().await?;
if health.is_healthy {
    println!("💚 Database is healthy!");
    println!("   - Uptime: {}s", health.uptime_seconds);
    println!("   - Active connections: {}", health.active_connections);
} else {
    println!("💔 Database health issues detected!");
    for issue in &health.issues {
        println!("   - ⚠️  {}", issue);
    }
}
```

## 🔧 Configuration Options

### **Performance Tuning**
```rust
let optimized_config = BlockDBConfig {
    data_dir: "./high_performance_db".to_string(),
    memtable_size_limit: 256 * 1024 * 1024, // 256MB for high throughput
    wal_sync_interval_ms: 500,               // Faster sync for durability
    compaction_threshold: 8,                 // Less frequent compaction
    blockchain_batch_size: 5000,             // Larger batches for efficiency
};
```

### **Security Configuration**
```rust
let secure_config = AuthConfig {
    enabled: true,
    session_duration_hours: 8,               // Shorter sessions
    max_failed_attempts: 3,                  // Account lockout
    password_min_length: 12,                 // Strong passwords
    require_strong_passwords: true,
    admin_users: vec!["admin".to_string()],
    allow_anonymous_reads: false,            // Require auth for all operations
    token_refresh_threshold_hours: 2,
};
```

## 🎯 Use Cases

### **1. Financial Ledger**
```rust
// Immutable financial transactions
db.put(b"tx:001", b"{'from':'alice','to':'bob','amount':100,'timestamp':'2024-01-01T10:00:00Z'}").await?;
db.put(b"tx:002", b"{'from':'bob','to':'charlie','amount':50,'timestamp':'2024-01-01T10:05:00Z'}").await?;

// Audit trail is automatically maintained in blockchain
let integrity_verified = db.verify_integrity().await?;
assert!(integrity_verified);
```

### **2. Event Sourcing**
```rust
// Store events in order
db.put(b"event:user_created:1001", b"{'user_id':1001,'name':'Alice','email':'alice@example.com'}").await?;
db.put(b"event:user_updated:1001", b"{'user_id':1001,'field':'email','new_value':'alice.smith@example.com'}").await?;
db.put(b"event:user_login:1001", b"{'user_id':1001,'timestamp':'2024-01-01T10:00:00Z','ip':'192.168.1.100'}").await?;
```

### **3. Configuration Management**
```rust
// Immutable configuration versions
db.put(b"config:v1", b"{'api_url':'http://api.v1.example.com','timeout':30}").await?;
db.put(b"config:v2", b"{'api_url':'http://api.v2.example.com','timeout':45}").await?;

// Previous configurations remain accessible
let v1_config = db.get(b"config:v1").await?.unwrap();
let v2_config = db.get(b"config:v2").await?.unwrap();
```

## 🎉 Key Features Demonstrated

✅ **Append-Only Architecture** - No updates or deletes, ensuring data immutability  
✅ **Blockchain Verification** - Cryptographic integrity for all operations  
✅ **High Performance** - Optimized LSM-tree storage with memory-resident writes  
✅ **Distributed Consensus** - Raft algorithm for multi-node consistency  
✅ **ACID Transactions** - Full transaction support with deadlock detection  
✅ **Authentication** - Blockchain-native user management and permissions  
✅ **Audit Trail** - Complete operation history with cryptographic verification  
✅ **Monitoring** - Comprehensive metrics and health checking  

## 🔗 Next Steps

1. **Start Simple**: Begin with single-node setup for development
2. **Add Nodes**: Scale to distributed cluster for production
3. **Enable Auth**: Add user management for secure access
4. **Monitor**: Use built-in metrics for performance optimization
5. **Integrate**: Connect with your application via the API

BlockDB provides a unique combination of blockchain verification, high performance, and enterprise-grade features for applications requiring immutable, auditable data storage.