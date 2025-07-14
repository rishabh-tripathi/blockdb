# BlockDB API Reference

## Overview

BlockDB provides multiple interfaces for interacting with the database:

1. **Command Line Interface (CLI)** - Direct terminal access
2. **HTTP REST API** - Web service endpoints (planned)
3. **Rust Library API** - Native Rust integration
4. **gRPC Service** - High-performance RPC (planned)

## CLI API Reference

### Global Options

All CLI commands support these global options:

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--data-dir` | `-d` | Database directory path | `./blockdb_data` |
| `--help` | `-h` | Show help information | - |
| `--version` | `-V` | Show version information | - |

### Commands

#### PUT Command

Store a key-value pair in the database.

**Syntax:**
```bash
blockdb-cli put <KEY> <VALUE> [FLAGS]
```

**Arguments:**
- `KEY` - The key to store (string)
- `VALUE` - The value to associate with the key (string)

**Flags:**
- `--base64` - Treat key and value as base64-encoded binary data

**Examples:**
```bash
# Store text data
blockdb-cli put "user:1001" "John Doe"

# Store with custom data directory
blockdb-cli -d /var/lib/blockdb put "config:timeout" "30"

# Store binary data (base64 encoded)
blockdb-cli put "YmluYXJ5X2tleQ==" "YmluYXJ5X3ZhbHVl" --base64
```

**Success Response:**
```
Successfully stored key-value pair
```

**Error Responses:**
```bash
# Duplicate key error (append-only enforcement)
Error: StorageError("Duplicate Key Error: Key 'user:1001' already exists. BlockDB is append-only and does not allow updates.")

# Invalid base64 encoding
Error: SerializationError("Invalid base64 encoding")

# Permission denied
Error: IoError("Permission denied (os error 13)")
```

#### GET Command

Retrieve a value by its key.

**Syntax:**
```bash
blockdb-cli get <KEY> [FLAGS]
```

**Arguments:**
- `KEY` - The key to retrieve (string)

**Flags:**
- `--base64` - Return result as base64-encoded data

**Examples:**
```bash
# Get text data
blockdb-cli get "user:1001"

# Get binary data as base64
blockdb-cli get "binary_key" --base64

# Get with custom data directory
blockdb-cli -d /var/lib/blockdb get "config:timeout"
```

**Success Response:**
```
John Doe
```

**Error Responses:**
```bash
# Key not found (returns empty, exit code 0)

# Permission denied
Error: IoError("Permission denied (os error 13)")

# Database corruption
Error: StorageError("Failed to read from storage")
```

#### STATS Command

Display database statistics and configuration.

**Syntax:**
```bash
blockdb-cli stats
```

**Example:**
```bash
blockdb-cli stats
```

**Response:**
```
BlockDB Statistics:
  Data directory: ./blockdb_data
  Memtable size limit: 64 MB
  WAL sync interval: 1000 ms
  Compaction threshold: 4
  Blockchain batch size: 1000
```

#### VERIFY Command

Verify blockchain integrity.

**Syntax:**
```bash
blockdb-cli verify
```

**Example:**
```bash
blockdb-cli verify
```

**Success Response:**
```
Verifying blockchain integrity...
✓ Blockchain integrity verified successfully
```

**Error Response:**
```
Verifying blockchain integrity...
✗ Blockchain integrity verification failed
Error: BlockchainError("Hash mismatch at block 42")
```

#### COLLECTION Commands

Manage collections within the database.

**Create Collection:**
```bash
blockdb-cli collection create <NAME> [--schema <SCHEMA_FILE>] [--description <DESC>]
```

**List Collections:**
```bash
blockdb-cli collection list
```

**Drop Collection:**
```bash
blockdb-cli collection drop <COLLECTION_ID>
```

**Collection Operations:**
```bash
# Store data in collection
blockdb-cli collection put <COLLECTION_ID> <KEY> <VALUE>

# Retrieve data from collection
blockdb-cli collection get <COLLECTION_ID> <KEY>

# List documents in collection
blockdb-cli collection list-docs <COLLECTION_ID> [--prefix <PREFIX>] [--limit <LIMIT>]

# Collection statistics
blockdb-cli collection stats <COLLECTION_ID>

# Verify collection integrity
blockdb-cli collection verify <COLLECTION_ID>
```

**Index Management:**
```bash
# Create index
blockdb-cli collection create-index <COLLECTION_ID> <INDEX_NAME> --fields <FIELD1,FIELD2> [--unique] [--sparse]

# Drop index
blockdb-cli collection drop-index <COLLECTION_ID> <INDEX_NAME>
```

**Examples:**
```bash
# Create a users collection
blockdb-cli collection create users --description "User data collection"

# Store user data
blockdb-cli collection put col_123 "user:1001" "John Doe"

# Retrieve user data
blockdb-cli collection get col_123 "user:1001"

# List all collections
blockdb-cli collection list

# Create an index on email field
blockdb-cli collection create-index col_123 email_index --fields email --unique

# Get collection statistics
blockdb-cli collection stats col_123

# Verify collection integrity
blockdb-cli collection verify col_123
```

#### INTERACTIVE Command

Start an interactive session for multiple operations.

**Syntax:**
```bash
blockdb-cli interactive
```

**Interactive Commands:**
- `put <key> <value>` - Store data
- `get <key>` - Retrieve data
- `collection create <name>` - Create collection
- `collection list` - List collections
- `collection put <id> <key> <value>` - Store in collection
- `collection get <id> <key>` - Retrieve from collection
- `stats` - Show statistics
- `verify` - Verify blockchain
- `help` - Show help
- `exit` - Exit interactive mode

**Example:**
```bash
blockdb-cli interactive

BlockDB Interactive Mode
Type 'help' for available commands, 'exit' to quit.

> collection create users
Collection 'users' created with ID: col_123

> collection put col_123 user:1001 "Alice"
Successfully stored document in collection

> collection get col_123 user:1001
Alice

> collection list
Collections:
  • users (col_123) - 1 documents, 15 bytes

> verify
✓ Blockchain integrity verified successfully

> exit
Goodbye!
```

## HTTP REST API Reference

### Base URL

```
http://localhost:8080/api/v1
```

### Authentication

Currently, BlockDB does not implement authentication. This will be added in future versions.

### Content Types

- Request: `application/json`
- Response: `application/json`

### Data Operations

#### Store Data

**Endpoint:** `POST /api/v1/put`

**Request Body:**
```json
{
  "key": "user:1001",
  "value": "John Doe",
  "encoding": "utf8"
}
```

**Response:**
```json
{
  "success": true,
  "message": "Data stored successfully"
}
```

**Error Response:**
```json
{
  "success": false,
  "error": "DuplicateKey",
  "message": "Key 'user:1001' already exists"
}
```

#### Retrieve Data

**Endpoint:** `GET /api/v1/get/{key}`

**Parameters:**
- `key` (path) - The key to retrieve
- `encoding` (query, optional) - Response encoding (`utf8`, `base64`)

**Example:**
```bash
GET /api/v1/get/user:1001?encoding=utf8
```

**Response:**
```json
{
  "success": true,
  "key": "user:1001",
  "value": "John Doe",
  "encoding": "utf8"
}
```

**Not Found Response:**
```json
{
  "success": false,
  "error": "NotFound",
  "message": "Key 'user:1001' not found"
}
```

### Collection Operations

#### Create Collection

**Endpoint:** `POST /api/v1/collections`

**Request Body:**
```json
{
  "name": "users",
  "description": "User data collection",
  "schema": {
    "version": 1,
    "fields": {
      "email": {
        "field_type": "String",
        "required": true,
        "validation_rules": [
          {"Pattern": "^[^@]+@[^@]+\\.[^@]+$"}
        ]
      },
      "name": {
        "field_type": "String",
        "required": true,
        "validation_rules": [
          {"MinLength": 1},
          {"MaxLength": 100}
        ]
      }
    },
    "required_fields": ["email", "name"],
    "indexes": [
      {
        "name": "email_index",
        "fields": ["email"],
        "unique": true,
        "sparse": false
      }
    ]
  },
  "settings": {
    "max_document_size": 16777216,
    "ttl_seconds": null,
    "compression_enabled": false,
    "encryption_enabled": false,
    "replication_factor": 3,
    "read_concern": "Local",
    "write_concern": "Acknowledged"
  }
}
```

**Response:**
```json
{
  "success": true,
  "collection_id": "col_550e8400-e29b-41d4-a716-446655440000",
  "message": "Collection 'users' created successfully"
}
```

**Error Response:**
```json
{
  "success": false,
  "error": "DuplicateCollection",
  "message": "Collection with name 'users' already exists"
}
```

#### List Collections

**Endpoint:** `GET /api/v1/collections`

**Response:**
```json
{
  "success": true,
  "collections": [
    {
      "id": "col_550e8400-e29b-41d4-a716-446655440000",
      "name": "users",
      "description": "User data collection",
      "created_at": 1705320000,
      "created_by": "admin",
      "stats": {
        "document_count": 1234,
        "total_size_bytes": 5678900,
        "index_size_bytes": 123456,
        "last_updated": 1705320000,
        "operations_count": 2468
      }
    },
    {
      "id": "col_550e8400-e29b-41d4-a716-446655440001",
      "name": "orders",
      "description": "Order data collection",
      "created_at": 1705320100,
      "created_by": "admin",
      "stats": {
        "document_count": 567,
        "total_size_bytes": 234567,
        "index_size_bytes": 12345,
        "last_updated": 1705320100,
        "operations_count": 890
      }
    }
  ]
}
```

#### Get Collection Details

**Endpoint:** `GET /api/v1/collections/{collection_id}`

**Parameters:**
- `collection_id` (path) - The collection ID

**Response:**
```json
{
  "success": true,
  "collection": {
    "id": "col_550e8400-e29b-41d4-a716-446655440000",
    "name": "users",
    "description": "User data collection",
    "created_at": 1705320000,
    "created_by": "admin",
    "schema": {
      "version": 1,
      "fields": {
        "email": {
          "field_type": "String",
          "required": true,
          "validation_rules": [
            {"Pattern": "^[^@]+@[^@]+\\.[^@]+$"}
          ]
        }
      },
      "required_fields": ["email"],
      "indexes": [
        {
          "name": "email_index",
          "fields": ["email"],
          "unique": true,
          "sparse": false
        }
      ]
    },
    "settings": {
      "max_document_size": 16777216,
      "ttl_seconds": null,
      "compression_enabled": false,
      "encryption_enabled": false,
      "replication_factor": 3,
      "read_concern": "Local",
      "write_concern": "Acknowledged"
    },
    "stats": {
      "document_count": 1234,
      "total_size_bytes": 5678900,
      "index_size_bytes": 123456,
      "last_updated": 1705320000,
      "operations_count": 2468
    }
  }
}
```

#### Drop Collection

**Endpoint:** `DELETE /api/v1/collections/{collection_id}`

**Parameters:**
- `collection_id` (path) - The collection ID

**Response:**
```json
{
  "success": true,
  "message": "Collection 'users' dropped successfully"
}
```

**Error Response:**
```json
{
  "success": false,
  "error": "CollectionNotFound",
  "message": "Collection 'col_123' not found"
}
```

#### Store Data in Collection

**Endpoint:** `POST /api/v1/collections/{collection_id}/documents`

**Parameters:**
- `collection_id` (path) - The collection ID

**Request Body:**
```json
{
  "key": "user:1001",
  "value": "John Doe",
  "encoding": "utf8"
}
```

**Response:**
```json
{
  "success": true,
  "message": "Document stored successfully in collection"
}
```

**Error Response:**
```json
{
  "success": false,
  "error": "ValidationError",
  "message": "Document validation failed: missing required field 'email'"
}
```

#### Retrieve Data from Collection

**Endpoint:** `GET /api/v1/collections/{collection_id}/documents/{key}`

**Parameters:**
- `collection_id` (path) - The collection ID
- `key` (path) - The document key
- `encoding` (query, optional) - Response encoding (`utf8`, `base64`)

**Response:**
```json
{
  "success": true,
  "collection_id": "col_550e8400-e29b-41d4-a716-446655440000",
  "key": "user:1001",
  "value": "John Doe",
  "encoding": "utf8"
}
```

#### List Documents in Collection

**Endpoint:** `GET /api/v1/collections/{collection_id}/documents`

**Parameters:**
- `collection_id` (path) - The collection ID
- `prefix` (query, optional) - Key prefix filter
- `limit` (query, optional) - Maximum number of results (default: 100)
- `offset` (query, optional) - Results offset for pagination

**Response:**
```json
{
  "success": true,
  "collection_id": "col_550e8400-e29b-41d4-a716-446655440000",
  "documents": [
    {
      "key": "user:1001",
      "value": "John Doe",
      "encoding": "utf8"
    },
    {
      "key": "user:1002",
      "value": "Jane Smith",
      "encoding": "utf8"
    }
  ],
  "total_count": 1234,
  "has_more": true
}
```

#### Create Index

**Endpoint:** `POST /api/v1/collections/{collection_id}/indexes`

**Parameters:**
- `collection_id` (path) - The collection ID

**Request Body:**
```json
{
  "name": "name_index",
  "fields": ["name"],
  "unique": false,
  "sparse": true
}
```

**Response:**
```json
{
  "success": true,
  "message": "Index 'name_index' created successfully"
}
```

#### Drop Index

**Endpoint:** `DELETE /api/v1/collections/{collection_id}/indexes/{index_name}`

**Parameters:**
- `collection_id` (path) - The collection ID
- `index_name` (path) - The index name

**Response:**
```json
{
  "success": true,
  "message": "Index 'name_index' dropped successfully"
}
```

#### Collection Statistics

**Endpoint:** `GET /api/v1/collections/{collection_id}/stats`

**Parameters:**
- `collection_id` (path) - The collection ID

**Response:**
```json
{
  "success": true,
  "collection_id": "col_550e8400-e29b-41d4-a716-446655440000",
  "stats": {
    "document_count": 1234,
    "total_size_bytes": 5678900,
    "index_size_bytes": 123456,
    "last_updated": 1705320000,
    "operations_count": 2468
  }
}
```

#### Verify Collection Integrity

**Endpoint:** `POST /api/v1/collections/{collection_id}/verify`

**Parameters:**
- `collection_id` (path) - The collection ID

**Response:**
```json
{
  "success": true,
  "collection_id": "col_550e8400-e29b-41d4-a716-446655440000",
  "blockchain_valid": true,
  "blocks_verified": 456,
  "verification_time_ms": 23
}
```

#### Batch Operations

**Endpoint:** `POST /api/v1/batch`

**Request Body:**
```json
{
  "operations": [
    {
      "type": "put",
      "key": "user:1001",
      "value": "John Doe"
    },
    {
      "type": "put",
      "key": "user:1002",
      "value": "Jane Smith"
    }
  ]
}
```

**Response:**
```json
{
  "success": true,
  "results": [
    {
      "operation": 0,
      "success": true
    },
    {
      "operation": 1,
      "success": true
    }
  ]
}
```

### Cluster Management

#### Cluster Status

**Endpoint:** `GET /cluster/status`

**Response:**
```json
{
  "cluster_id": "production-cluster",
  "node_id": "node1",
  "state": "Leader",
  "term": 42,
  "leader": "node1",
  "nodes": [
    {
      "id": "node1",
      "address": "192.168.1.10:8080",
      "state": "Leader",
      "last_heartbeat": "2024-01-15T10:30:00Z"
    },
    {
      "id": "node2",
      "address": "192.168.1.11:8080",
      "state": "Follower",
      "last_heartbeat": "2024-01-15T10:29:58Z"
    }
  ]
}
```

#### Add Node

**Endpoint:** `POST /cluster/add`

**Request Body:**
```json
{
  "node_id": "node3",
  "address": "192.168.1.12:8080"
}
```

**Response:**
```json
{
  "success": true,
  "message": "Node added successfully"
}
```

#### Remove Node

**Endpoint:** `DELETE /cluster/remove/{node_id}`

**Response:**
```json
{
  "success": true,
  "message": "Node removed successfully"
}
```

### System Operations

#### Health Check

**Endpoint:** `GET /health`

**Response:**
```json
{
  "status": "healthy",
  "uptime": "2h 45m 30s",
  "version": "0.1.0"
}
```

#### Metrics

**Endpoint:** `GET /metrics`

**Response:**
```json
{
  "operations": {
    "total_reads": 1234,
    "total_writes": 567,
    "failed_operations": 3
  },
  "performance": {
    "avg_read_latency_ms": 0.8,
    "avg_write_latency_ms": 5.2,
    "throughput_ops_per_sec": 190.5
  },
  "storage": {
    "memtable_size_bytes": 12345678,
    "total_sstables": 5,
    "blockchain_blocks": 123
  },
  "cluster": {
    "consensus_state": "Leader",
    "active_nodes": 3,
    "last_election": "2024-01-15T08:15:30Z"
  }
}
```

#### Database Statistics

**Endpoint:** `GET /stats`

**Response:**
```json
{
  "config": {
    "data_dir": "./blockdb_data",
    "memtable_size_limit": 67108864,
    "wal_sync_interval": 1000,
    "compaction_threshold": 4,
    "blockchain_batch_size": 1000
  },
  "storage": {
    "memtable_entries": 1234,
    "sstable_count": 5,
    "wal_size_bytes": 2345678,
    "total_size_bytes": 12345678
  }
}
```

#### Verify Integrity

**Endpoint:** `POST /verify`

**Response:**
```json
{
  "success": true,
  "blockchain_valid": true,
  "blocks_verified": 123,
  "verification_time_ms": 45
}
```

## Rust Library API

### Basic Usage

```rust
use blockdb::{
    BlockDBConfig, BlockDBHandle, DistributedBlockDB, DistributedBlockDBConfig,
    CollectionManager, CollectionMetadata, CollectionSchema, FieldDefinition, FieldType
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Single node setup
    let config = BlockDBConfig {
        data_dir: "./my_data".to_string(),
        memtable_size_limit: 64 * 1024 * 1024,
        wal_sync_interval_ms: 1000,
        compaction_threshold: 4,
        blockchain_batch_size: 1000,
    };
    
    let db = BlockDBHandle::new(config.clone())?;
    
    // Basic operations
    db.put(b"key1", b"value1").await?;
    let value = db.get(b"key1").await?;
    
    // Collection operations
    let collection_manager = CollectionManager::new(config.clone())?;
    
    // Create a collection
    let users_collection_id = collection_manager.create_collection(
        "users".to_string(),
        None,  // No schema
        None,  // Default settings
        Some("admin".to_string())
    )?;
    
    // Store data in collection
    collection_manager.put(&users_collection_id, b"user:1001", b"Alice Smith")?;
    
    // Retrieve data from collection
    let user_data = collection_manager.get(&users_collection_id, b"user:1001")?;
    
    // List all collections
    let collections = collection_manager.list_collections()?;
    
    // Get collection statistics
    let stats = collection_manager.get_collection_stats(&users_collection_id)?;
    
    // Verify collection integrity
    let is_valid = collection_manager.verify_all_integrity()?;
    
    // Distributed setup
    let distributed_config = DistributedBlockDBConfig::default();
    let distributed_db = DistributedBlockDB::new(distributed_config).await?;
    
    // Distributed operations
    distributed_db.put(b"distributed_key", b"distributed_value").await?;
    let distributed_value = distributed_db.get(b"distributed_key").await?;
    
    Ok(())
}
```

### Collection Usage Examples

```rust
use blockdb::storage::collection::{
    CollectionManager, CollectionMetadata, CollectionSchema, 
    FieldDefinition, FieldType, ValidationRule, IndexDefinition
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = BlockDBConfig::default();
    let manager = CollectionManager::new(config)?;
    
    // Create collection with schema
    let mut schema = CollectionSchema {
        version: 1,
        fields: std::collections::HashMap::new(),
        required_fields: vec!["email".to_string(), "name".to_string()],
        indexes: Vec::new(),
    };
    
    // Define email field with validation
    schema.fields.insert("email".to_string(), FieldDefinition {
        field_type: FieldType::String,
        required: true,
        default_value: None,
        validation_rules: vec![
            ValidationRule::Pattern("^[^@]+@[^@]+\\.[^@]+$".to_string())
        ],
    });
    
    // Define name field
    schema.fields.insert("name".to_string(), FieldDefinition {
        field_type: FieldType::String,
        required: true,
        default_value: None,
        validation_rules: vec![
            ValidationRule::MinLength(1),
            ValidationRule::MaxLength(100)
        ],
    });
    
    // Create collection with schema
    let users_id = manager.create_collection(
        "users".to_string(),
        Some(schema),
        None,
        Some("admin".to_string())
    )?;
    
    // Create index on email field
    let email_index = IndexDefinition {
        name: "email_index".to_string(),
        fields: vec!["email".to_string()],
        unique: true,
        sparse: false,
    };
    manager.create_index(&users_id, email_index)?;
    
    // Store user data
    manager.put(&users_id, b"user:1001", b"alice@example.com")?;
    manager.put(&users_id, b"user:1002", b"bob@example.com")?;
    
    // Retrieve user data
    let alice_data = manager.get(&users_id, b"user:1001")?;
    
    // Multi-tenant example
    let tenant_a_id = manager.create_collection(
        "tenant_a_data".to_string(),
        None,
        None,
        Some("tenant_a".to_string())
    )?;
    
    let tenant_b_id = manager.create_collection(
        "tenant_b_data".to_string(),
        None,
        None,
        Some("tenant_b".to_string())
    )?;
    
    // Store tenant-specific data
    manager.put(&tenant_a_id, b"config:api_limit", b"1000")?;
    manager.put(&tenant_b_id, b"config:api_limit", b"5000")?;
    
    // Verify isolation
    let tenant_a_config = manager.get(&tenant_a_id, b"config:api_limit")?;
    let tenant_b_config = manager.get(&tenant_b_id, b"config:api_limit")?;
    
    // Get aggregate statistics
    let (total_collections, total_documents, total_size) = manager.get_total_stats()?;
    
    println!("Collections: {}, Documents: {}, Size: {} bytes", 
             total_collections, total_documents, total_size);
    
    Ok(())
}
```

### Configuration

#### BlockDBConfig

```rust
pub struct BlockDBConfig {
    pub data_dir: String,                    // Database directory
    pub memtable_size_limit: usize,          // MemTable size in bytes
    pub wal_sync_interval: u64,              // WAL sync interval in ms
    pub compaction_threshold: usize,         // SSTable compaction threshold
    pub blockchain_batch_size: usize,        // Blockchain batch size
}

impl Default for BlockDBConfig {
    fn default() -> Self {
        BlockDBConfig {
            data_dir: "./blockdb_data".to_string(),
            memtable_size_limit: 64 * 1024 * 1024,  // 64MB
            wal_sync_interval: 1000,                 // 1 second
            compaction_threshold: 4,                 // 4 SSTables
            blockchain_batch_size: 1000,             // 1000 records
        }
    }
}
```

#### DistributedBlockDBConfig

```rust
pub struct DistributedBlockDBConfig {
    pub storage_config: BlockDBConfig,
    pub cluster_config: ClusterConfig,
    pub enable_transactions: bool,
    pub transaction_timeout: Duration,
    pub consensus_timeout: Duration,
}
```

### Core Methods

#### BlockDBHandle

```rust
impl BlockDBHandle {
    // Create new database instance
    pub fn new(config: BlockDBConfig) -> Result<Self, BlockDBError>;
    
    // Store key-value pair
    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<(), BlockDBError>;
    
    // Retrieve value by key
    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, BlockDBError>;
    
    // Verify blockchain integrity
    pub async fn verify_integrity(&self) -> Result<bool, BlockDBError>;
    
    // Force flush memtable to disk
    pub async fn force_flush(&self) -> Result<(), BlockDBError>;
}
```

#### DistributedBlockDB

```rust
impl DistributedBlockDB {
    // Create distributed database instance
    pub async fn new(config: DistributedBlockDBConfig) -> Result<Self, BlockDBError>;
    
    // Start the distributed database
    pub async fn start(&self) -> Result<(), BlockDBError>;
    
    // Stop the distributed database
    pub async fn stop(&self) -> Result<(), BlockDBError>;
    
    // Distributed put operation
    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<(), BlockDBError>;
    
    // Distributed get operation
    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, BlockDBError>;
    
    // Transaction operations
    pub async fn begin_transaction(&self) -> Result<TransactionId, BlockDBError>;
    pub async fn commit_transaction(&self, tx_id: &TransactionId) -> Result<(), BlockDBError>;
    pub async fn abort_transaction(&self, tx_id: &TransactionId) -> Result<(), BlockDBError>;
    
    // Cluster management
    pub async fn add_node(&self, node_id: NodeId, address: NodeAddress) -> Result<(), BlockDBError>;
    pub async fn remove_node(&self, node_id: &NodeId) -> Result<(), BlockDBError>;
    pub async fn is_leader(&self) -> bool;
    pub async fn get_leader(&self) -> Option<NodeId>;
}
```

#### CollectionManager

```rust
impl CollectionManager {
    // Create new collection manager instance
    pub fn new(config: BlockDBConfig) -> Result<Self, BlockDBError>;
    
    // Collection lifecycle management
    pub fn create_collection(
        &self,
        name: String,
        schema: Option<CollectionSchema>,
        settings: Option<CollectionSettings>,
        created_by: Option<String>
    ) -> Result<CollectionId, BlockDBError>;
    
    pub fn drop_collection(&self, collection_id: &str) -> Result<(), BlockDBError>;
    
    pub fn get_collection(&self, collection_id: &str) -> Result<Collection, BlockDBError>;
    
    pub fn list_collections(&self) -> Result<Vec<CollectionMetadata>, BlockDBError>;
    
    pub fn collection_exists(&self, collection_id: &str) -> bool;
    
    pub fn get_collection_by_name(&self, name: &str) -> Result<Option<CollectionId>, BlockDBError>;
    
    // Data operations
    pub fn put(&self, collection_id: &str, key: &[u8], value: &[u8]) -> Result<(), BlockDBError>;
    
    pub fn get(&self, collection_id: &str, key: &[u8]) -> Result<Option<Vec<u8>>, BlockDBError>;
    
    pub fn delete(&self, collection_id: &str, key: &[u8]) -> Result<(), BlockDBError>;
    
    pub fn list_keys(
        &self,
        collection_id: &str,
        prefix: Option<&[u8]>,
        limit: Option<usize>
    ) -> Result<Vec<Vec<u8>>, BlockDBError>;
    
    // Statistics and monitoring
    pub fn get_collection_stats(&self, collection_id: &str) -> Result<CollectionStats, BlockDBError>;
    
    pub fn get_total_stats(&self) -> Result<(usize, u64, u64), BlockDBError>;
    
    pub fn verify_all_integrity(&self) -> Result<bool, BlockDBError>;
    
    // Index management
    pub fn create_index(
        &self,
        collection_id: &str,
        index_def: IndexDefinition
    ) -> Result<(), BlockDBError>;
    
    pub fn drop_index(&self, collection_id: &str, index_name: &str) -> Result<(), BlockDBError>;
}
```

### Error Handling

```rust
#[derive(Debug)]
pub enum BlockDBError {
    IoError(std::io::Error),
    SerializationError(bincode::Error),
    InvalidData(String),
    BlockchainError(String),
    StorageError(String),
    ApiError(String),
    DuplicateKey(String),
}
```

### Transaction API

```rust
// Execute multiple operations atomically
let result = distributed_db.execute_transaction(|ctx| async move {
    ctx.put(b"key1", b"value1").await?;
    ctx.put(b"key2", b"value2").await?;
    let value = ctx.get(b"key1").await?;
    Ok(value)
}).await?;
```

## Performance Considerations

### Throughput Optimization

1. **Batch Operations**: Use batch endpoints for multiple operations
2. **MemTable Tuning**: Increase memtable size for write-heavy workloads
3. **WAL Configuration**: Adjust sync interval based on durability requirements
4. **Compaction Tuning**: Optimize compaction threshold for storage patterns

### Latency Optimization

1. **Read Caching**: Enable read caching for frequently accessed data
2. **Local Reads**: Use local read operations in distributed setup
3. **Network Optimization**: Optimize network configuration for cluster communication
4. **SSD Storage**: Use SSDs for optimal I/O performance

### Memory Management

1. **MemTable Size**: Configure based on available memory
2. **Connection Pooling**: Reuse connections for HTTP API
3. **Background Tasks**: Monitor background task resource usage

## Security Considerations

### Data Protection

1. **Directory Permissions**: Secure database directory access
2. **Network Security**: Use firewalls and VPNs for cluster communication
3. **Audit Logging**: Enable audit logs for security monitoring

### Access Control (Future)

1. **Authentication**: Token-based authentication (planned)
2. **Authorization**: Role-based access control (planned)
3. **Encryption**: Data encryption at rest and in transit (planned)

## Monitoring and Observability

### Metrics Collection

Monitor these key metrics:

1. **Performance Metrics**:
   - Read/write throughput
   - Operation latency
   - Error rates

2. **Storage Metrics**:
   - MemTable utilization
   - SSTable count
   - WAL size
   - Compaction frequency

3. **Cluster Metrics**:
   - Leader election frequency
   - Node health status
   - Consensus latency

### Health Checks

Implement regular health checks:

1. **Database Health**: Verify basic operations
2. **Storage Health**: Check disk space and I/O
3. **Cluster Health**: Monitor node connectivity
4. **Blockchain Integrity**: Regular integrity verification

## Best Practices

### Application Design

1. **Key Design**: Use hierarchical key patterns (`user:id`, `config:section:key`)
2. **Value Size**: Keep values reasonably sized (< 1MB recommended)
3. **Error Handling**: Implement proper error handling for all operations
4. **Resource Management**: Close connections and clean up resources

### Operational Best Practices

1. **Backup Strategy**: Regular blockchain integrity verification
2. **Monitoring**: Comprehensive monitoring and alerting
3. **Capacity Planning**: Monitor growth and plan for scaling
4. **Testing**: Test failure scenarios and recovery procedures

### Development Best Practices

1. **Configuration Management**: Use configuration files for settings
2. **Testing**: Unit tests and integration tests
3. **Documentation**: Keep API usage documented
4. **Version Management**: Plan for schema evolution