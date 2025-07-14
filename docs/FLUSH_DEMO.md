# BlockDB Flush Functionality Demo

This document demonstrates the new flush functionality implemented in BlockDB for clearing all data from collections or the entire database.

## Features Implemented

### 1. **Global Database Flush**
- Clears all data from the entire database
- Removes all MemTable entries
- Clears Write-Ahead Log (WAL)
- Removes all SSTable files
- Resets blockchain to genesis block
- Resets sequence counter
- Requires confirmation to prevent accidental data loss

### 2. **Collection Flush**
- Clears all data from a specific collection
- Resets collection statistics
- Clears collection indexes
- Maintains collection metadata structure
- Requires confirmation to prevent accidental data loss

### 3. **Force Flush Options**
- `--force` flag bypasses confirmation prompts
- Useful for automated scripts and CI/CD pipelines

## CLI Commands

### Global Database Flush
```bash
# Interactive flush with confirmation
blockdb-cli flush

# Force flush without confirmation
blockdb-cli flush --force
```

### Collection Flush
```bash
# Interactive flush of a specific collection
blockdb-cli collection flush <collection_id>

# Force flush of a collection
blockdb-cli collection flush <collection_id> --force
```

### Interactive Mode
```bash
# Start interactive mode
blockdb-cli interactive

# In interactive mode:
blockdb> flush                              # Flush entire database
blockdb> collection flush <collection_id>   # Flush specific collection
```

## Usage Examples

### Example 1: Basic Database Operations with Flush

```bash
# Create some test data
blockdb-cli put "user:1" "Alice"
blockdb-cli put "user:2" "Bob"
blockdb-cli put "product:1" "Laptop"

# Verify data exists
blockdb-cli get "user:1"
# Output: Alice

blockdb-cli get "product:1"
# Output: Laptop

# Check database statistics
blockdb-cli stats
# Output: Shows database configuration and status

# Flush all data (with confirmation)
blockdb-cli flush
# Output: ⚠️  WARNING: This will delete ALL data in the database!
#         Are you sure you want to continue? (y/N): y
#         Flushing all database data...
#         ✅ Database flushed successfully

# Verify data is gone
blockdb-cli get "user:1"
# Output: Key not found
```

### Example 2: Collection Operations with Flush

```bash
# Create collections
blockdb-cli collection create users --description "User data"
blockdb-cli collection create products --description "Product catalog"

# Add data to collections
blockdb-cli collection put col_xxx "user:1" "Alice"
blockdb-cli collection put col_xxx "user:2" "Bob"
blockdb-cli collection put col_yyy "product:1" "Laptop"

# List collections
blockdb-cli collection list
# Output: Collections:
#         • users (col_xxx) - 2 documents, 15 bytes
#         • products (col_yyy) - 1 documents, 6 bytes

# Flush specific collection
blockdb-cli collection flush col_xxx
# Output: ⚠️  WARNING: This will delete ALL data in collection 'col_xxx'!
#         Are you sure you want to continue? (y/N): y
#         ✅ Collection 'col_xxx' flushed successfully

# Verify collection data is gone but other collections remain
blockdb-cli collection get col_xxx "user:1"
# Output: Document not found in collection

blockdb-cli collection get col_yyy "product:1"
# Output: Laptop
```

### Example 3: Interactive Mode with Flush

```bash
blockdb-cli interactive
# Output: BlockDB Interactive Mode
#         Commands: put <key> <value>, get <key>, stats, verify, flush, collection <action>, quit

blockdb> put "test" "value"
# Output: OK

blockdb> get "test"
# Output: value

blockdb> collection create demo
# Output: ✅ Collection 'demo' created with ID: col_123

blockdb> collection put col_123 "demo" "data"
# Output: OK

blockdb> collection list
# Output: Collections:
#         • demo (col_123) - 1 documents, 4 bytes

blockdb> collection flush col_123
# Output: Are you sure you want to flush collection 'col_123'? (y/N): y
#         ✅ Collection flushed successfully

blockdb> flush
# Output: Are you sure you want to flush all data? (y/N): y
#         ✅ Database flushed successfully

blockdb> exit
# Output: Goodbye!
```

## Architecture Overview

### Storage Layer Changes
- **BlockDB::flush_all()**: Clears entire database state
- **BlockDB::force_flush_memtable()**: Forces memtable flush to disk
- **WAL::clear()**: Clears write-ahead log
- **BlockChain::clear()**: Resets blockchain to genesis state

### Collection Layer Changes
- **Collection::flush()**: Clears collection data and resets statistics
- **CollectionManager::flush_collection()**: Flushes specific collection
- **CollectionManager::flush_all()**: Flushes all collections

### CLI Layer Changes
- **Flush command**: Global database flush with confirmation
- **Collection flush subcommand**: Per-collection flush with confirmation
- **Interactive mode**: Flush commands in interactive shell
- **Force flags**: Bypass confirmation prompts

## Safety Features

### 1. **Confirmation Prompts**
- All flush operations require user confirmation by default
- Clear warning messages about data loss
- Case-insensitive 'y' or 'yes' required to proceed

### 2. **Force Bypass**
- `--force` flag available for automated scenarios
- Clearly documented to prevent accidental use

### 3. **Graceful Error Handling**
- Proper error messages for missing collections
- Safe handling of I/O errors during flush operations
- Transaction rollback on partial failures

## Performance Considerations

### 1. **Flush Performance**
- Fast in-memory structure clearing
- Efficient file system operations
- Minimal disk I/O for cleanup

### 2. **Recovery Performance**
- Quick restart after flush operations
- Efficient genesis block recreation
- Fast empty state initialization

## Use Cases

### 1. **Development and Testing**
- Quick database reset between test runs
- Clean state for integration tests
- Development environment cleanup

### 2. **Maintenance Operations**
- Database migration preparation
- Storage reclaim operations
- System maintenance windows

### 3. **Automated Operations**
- CI/CD pipeline database reset
- Scheduled cleanup operations
- Automated testing workflows

## Technical Implementation

### Core Components Modified

1. **Storage Engine** (`src/storage/mod.rs`)
   - Added `flush_all()` method for complete database clearing
   - Added `force_flush_memtable()` for explicit memtable flushing

2. **Write-Ahead Log** (`src/storage/wal.rs`)
   - Added `clear()` method for WAL reset

3. **Blockchain** (`src/storage/blockchain.rs`)
   - Added `clear()` method for blockchain reset to genesis

4. **Collection System** (`src/storage/collection.rs`)
   - Added `flush()` method to Collection
   - Added `flush_collection()` and `flush_all()` to CollectionManager

5. **CLI Interface** (`src/bin/simple_cli.rs`)
   - Added flush command with confirmation
   - Added collection flush subcommand
   - Added interactive mode flush support

### Error Handling
- Comprehensive error propagation
- Safe resource cleanup on failures
- Clear error messages for users

### Testing Considerations
- Unit tests for flush operations
- Integration tests for CLI commands
- Performance benchmarks for large datasets

## Future Enhancements

### 1. **Selective Flush**
- Flush by key prefix
- Flush by timestamp range
- Flush by collection criteria

### 2. **Backup Integration**
- Backup before flush operations
- Restore from backup functionality
- Incremental flush operations

### 3. **Monitoring and Metrics**
- Flush operation metrics
- Performance monitoring
- Audit trail for flush operations

## Conclusion

The flush functionality provides a powerful and safe way to clear data from BlockDB while maintaining data integrity and providing clear safety mechanisms. The implementation follows BlockDB's design principles of safety, performance, and ease of use.

Key benefits:
- ✅ Complete data clearing capability
- ✅ Per-collection granular control
- ✅ Safety confirmations prevent accidents
- ✅ Force mode for automation
- ✅ Interactive and command-line interfaces
- ✅ Proper error handling and recovery
- ✅ Efficient performance characteristics