BlockDB Flush Functionality - Manual Testing Guide

  This document provides a comprehensive step-by-step manual testing guide for all BlockDB flush functionality, including basic
  operations, collection management, and database administration.

  Prerequisites

  # Build the project
  cargo build --release

  # Make CLI executable easily accessible
  alias blockdb-cli="./target/release/blockdb-cli"

  # Create a clean testing directory
  mkdir -p /tmp/blockdb-test
  cd /tmp/blockdb-test

  Test Plan Overview

  1. Basic Database Operations
  2. Collection Management
  3. Global Database Flush
  4. Collection-Specific Flush
  5. Interactive Mode Testing
  6. Force Flag Testing
  7. Error Handling & Edge Cases

  ---
  1. Basic Database Operations Testing

  Step 1.1: Initialize and Add Basic Data

  # Set data directory for testing
  export BLOCKDB_DATA_DIR="/tmp/blockdb-test/data"

  # Check initial database stats
  blockdb-cli -d $BLOCKDB_DATA_DIR stats

  # Add some basic key-value pairs
  blockdb-cli -d $BLOCKDB_DATA_DIR put "user:1001" "Alice Johnson"
  blockdb-cli -d $BLOCKDB_DATA_DIR put "user:1002" "Bob Smith"
  blockdb-cli -d $BLOCKDB_DATA_DIR put "user:1003" "Charlie Brown"
  blockdb-cli -d $BLOCKDB_DATA_DIR put "product:2001" "Laptop"
  blockdb-cli -d $BLOCKDB_DATA_DIR put "product:2002" "Mouse"
  blockdb-cli -d $BLOCKDB_DATA_DIR put "order:3001" "Order for Alice"

  Step 1.2: Verify Data Retrieval

  # Retrieve individual records
  blockdb-cli -d $BLOCKDB_DATA_DIR get "user:1001"
  # Expected: Alice Johnson

  blockdb-cli -d $BLOCKDB_DATA_DIR get "product:2001"
  # Expected: Laptop

  blockdb-cli -d $BLOCKDB_DATA_DIR get "order:3001"
  # Expected: Order for Alice

  # Try to retrieve non-existent key
  blockdb-cli -d $BLOCKDB_DATA_DIR get "user:9999"
  # Expected: Key not found

  Step 1.3: Test Database Statistics and Verification

  # Check database statistics
  blockdb-cli -d $BLOCKDB_DATA_DIR stats

  # Verify blockchain integrity
  blockdb-cli -d $BLOCKDB_DATA_DIR verify
  # Expected: ✓ Blockchain integrity verified successfully

  ---
  2. Collection Management Testing

  Step 2.1: Create Collections

  # Create collections for different data types
  blockdb-cli -d $BLOCKDB_DATA_DIR collection create users --description "User accounts"
  blockdb-cli -d $BLOCKDB_DATA_DIR collection create products --description "Product catalog"
  blockdb-cli -d $BLOCKDB_DATA_DIR collection create orders --description "Customer orders"

  # List all collections
  blockdb-cli -d $BLOCKDB_DATA_DIR collection list
  # Expected: Shows 3 collections with their IDs

  Step 2.2: Add Data to Collections

  # Note: Replace col_xxx with actual collection IDs from previous step
  # Get collection IDs from the list command output

  # Add user data to users collection
  blockdb-cli -d $BLOCKDB_DATA_DIR collection put col_<users_id> "user:1001" "Alice Johnson - Manager"
  blockdb-cli -d $BLOCKDB_DATA_DIR collection put col_<users_id> "user:1002" "Bob Smith - Developer"
  blockdb-cli -d $BLOCKDB_DATA_DIR collection put col_<users_id> "user:1003" "Charlie Brown - Designer"

  # Add product data to products collection
  blockdb-cli -d $BLOCKDB_DATA_DIR collection put col_<products_id> "product:2001" "MacBook Pro 16-inch"
  blockdb-cli -d $BLOCKDB_DATA_DIR collection put col_<products_id> "product:2002" "Wireless Mouse"
  blockdb-cli -d $BLOCKDB_DATA_DIR collection put col_<products_id> "product:2003" "USB-C Cable"

  # Add order data to orders collection
  blockdb-cli -d $BLOCKDB_DATA_DIR collection put col_<orders_id> "order:3001" "Order #1001 - Alice - MacBook"
  blockdb-cli -d $BLOCKDB_DATA_DIR collection put col_<orders_id> "order:3002" "Order #1002 - Bob - Mouse"

  Step 2.3: Verify Collection Data

  # Retrieve data from collections
  blockdb-cli -d $BLOCKDB_DATA_DIR collection get col_<users_id> "user:1001"
  # Expected: Alice Johnson - Manager

  blockdb-cli -d $BLOCKDB_DATA_DIR collection get col_<products_id> "product:2001"
  # Expected: MacBook Pro 16-inch

  blockdb-cli -d $BLOCKDB_DATA_DIR collection get col_<orders_id> "order:3001"
  # Expected: Order #1001 - Alice - MacBook

  # Check collection statistics
  blockdb-cli -d $BLOCKDB_DATA_DIR collection stats col_<users_id>
  blockdb-cli -d $BLOCKDB_DATA_DIR collection stats col_<products_id>
  blockdb-cli -d $BLOCKDB_DATA_DIR collection stats col_<orders_id>

  # Verify collection integrity
  blockdb-cli -d $BLOCKDB_DATA_DIR collection verify col_<users_id>
  # Expected: ✓ Collection integrity verified successfully

  Step 2.4: Test Collection Indexes

  # Create indexes on collections
  blockdb-cli -d $BLOCKDB_DATA_DIR collection create-index col_<users_id> user_index --fields name --unique
  blockdb-cli -d $BLOCKDB_DATA_DIR collection create-index col_<products_id> product_index --fields name

  # Verify indexes were created (check collection stats)
  blockdb-cli -d $BLOCKDB_DATA_DIR collection stats col_<users_id>

  ---
  3. Global Database Flush Testing

  Step 3.1: Pre-Flush Verification

  # Verify we have data before flush
  echo "=== PRE-FLUSH DATA VERIFICATION ==="
  blockdb-cli -d $BLOCKDB_DATA_DIR get "user:1001"
  blockdb-cli -d $BLOCKDB_DATA_DIR get "product:2001"
  blockdb-cli -d $BLOCKDB_DATA_DIR collection list
  blockdb-cli -d $BLOCKDB_DATA_DIR collection get col_<users_id> "user:1001"
  blockdb-cli -d $BLOCKDB_DATA_DIR stats

  Step 3.2: Test Flush with Confirmation

  # Test flush with confirmation prompt
  blockdb-cli -d $BLOCKDB_DATA_DIR flush
  # Expected: ⚠️  WARNING: This will delete ALL data in the database!
  #           Are you sure you want to continue? (y/N):

  # Type 'n' to cancel
  # Expected: Operation cancelled.

  # Run flush again and type 'y' to confirm
  blockdb-cli -d $BLOCKDB_DATA_DIR flush
  # Type 'y' when prompted
  # Expected: Flushing all database data...
  #           ✅ Database flushed successfully

  Step 3.3: Post-Flush Verification

  # Verify all data is gone
  echo "=== POST-FLUSH VERIFICATION ==="
  blockdb-cli -d $BLOCKDB_DATA_DIR get "user:1001"
  # Expected: Key not found

  blockdb-cli -d $BLOCKDB_DATA_DIR get "product:2001"
  # Expected: Key not found

  blockdb-cli -d $BLOCKDB_DATA_DIR collection list
  # Expected: No collections found.

  # Check that database still works (add new data)
  blockdb-cli -d $BLOCKDB_DATA_DIR put "test:1" "test value"
  blockdb-cli -d $BLOCKDB_DATA_DIR get "test:1"
  # Expected: test value

  # Verify blockchain integrity after flush
  blockdb-cli -d $BLOCKDB_DATA_DIR verify
  # Expected: ✓ Blockchain integrity verified successfully

  ---
  4. Collection-Specific Flush Testing

  Step 4.1: Recreate Test Data

  # Create collections again
  blockdb-cli -d $BLOCKDB_DATA_DIR collection create users --description "User accounts"
  blockdb-cli -d $BLOCKDB_DATA_DIR collection create products --description "Product catalog"
  blockdb-cli -d $BLOCKDB_DATA_DIR collection create orders --description "Customer orders"

  # Add data to collections (replace col_xxx with actual IDs)
  blockdb-cli -d $BLOCKDB_DATA_DIR collection put col_<users_id> "user:1001" "Alice Johnson"
  blockdb-cli -d $BLOCKDB_DATA_DIR collection put col_<users_id> "user:1002" "Bob Smith"
  blockdb-cli -d $BLOCKDB_DATA_DIR collection put col_<products_id> "product:2001" "MacBook Pro"
  blockdb-cli -d $BLOCKDB_DATA_DIR collection put col_<products_id> "product:2002" "Wireless Mouse"
  blockdb-cli -d $BLOCKDB_DATA_DIR collection put col_<orders_id> "order:3001" "Order #1001"

  # Verify data exists
  blockdb-cli -d $BLOCKDB_DATA_DIR collection list

  Step 4.2: Test Collection Flush with Confirmation

  # Test collection flush with confirmation
  blockdb-cli -d $BLOCKDB_DATA_DIR collection flush col_<users_id>
  # Expected: ⚠️  WARNING: This will delete ALL data in collection 'col_xxx'!
  #           Are you sure you want to continue? (y/N):

  # Type 'n' to cancel
  # Expected: Operation cancelled.

  # Run flush again and type 'y' to confirm
  blockdb-cli -d $BLOCKDB_DATA_DIR collection flush col_<users_id>
  # Type 'y' when prompted
  # Expected: ✅ Collection 'col_xxx' flushed successfully

  Step 4.3: Verify Selective Flush

  # Verify users collection is empty
  blockdb-cli -d $BLOCKDB_DATA_DIR collection get col_<users_id> "user:1001"
  # Expected: Document not found in collection

  blockdb-cli -d $BLOCKDB_DATA_DIR collection stats col_<users_id>
  # Expected: Document count: 0

  # Verify other collections still have data
  blockdb-cli -d $BLOCKDB_DATA_DIR collection get col_<products_id> "product:2001"
  # Expected: MacBook Pro

  blockdb-cli -d $BLOCKDB_DATA_DIR collection get col_<orders_id> "order:3001"
  # Expected: Order #1001

  # Verify collection list shows all collections
  blockdb-cli -d $BLOCKDB_DATA_DIR collection list
  # Expected: Shows 3 collections, users collection with 0 documents

  ---
  5. Interactive Mode Testing

  Step 5.1: Basic Interactive Mode

  # Start interactive mode
  blockdb-cli -d $BLOCKDB_DATA_DIR interactive

  # In interactive mode, run these commands:

  BlockDB Interactive Mode
  Commands: put <key> <value>, get <key>, stats, verify, flush, collection <action>, quit

  blockdb> put "interactive:1" "test value"
  # Expected: OK

  blockdb> get "interactive:1"
  # Expected: test value

  blockdb> stats
  # Expected: Shows database statistics

  blockdb> verify
  # Expected: ✓ OK

  Step 5.2: Interactive Collection Commands

  blockdb> collection create test_interactive
  # Expected: ✅ Collection 'test_interactive' created with ID: col_xxx

  blockdb> collection list
  # Expected: Shows all collections including the new one

  blockdb> collection put col_<test_interactive_id> "test:1" "interactive test data"
  # Expected: OK

  blockdb> collection get col_<test_interactive_id> "test:1"
  # Expected: interactive test data

  blockdb> collection stats col_<test_interactive_id>
  # Expected: Shows collection statistics

  Step 5.3: Interactive Flush Commands

  blockdb> collection flush col_<test_interactive_id>
  # Expected: Are you sure you want to flush collection 'col_xxx'? (y/N): y
  # Expected: ✅ Collection flushed successfully

  blockdb> collection get col_<test_interactive_id> "test:1"
  # Expected: (nil)

  blockdb> flush
  # Expected: Are you sure you want to flush all data? (y/N): y
  # Expected: ✅ Database flushed successfully

  blockdb> collection list
  # Expected: No collections found.

  blockdb> exit
  # Expected: Goodbye!

  ---
  6. Force Flag Testing

  Step 6.1: Setup Test Data

  # Add some test data
  blockdb-cli -d $BLOCKDB_DATA_DIR put "force:1" "test data 1"
  blockdb-cli -d $BLOCKDB_DATA_DIR put "force:2" "test data 2"
  blockdb-cli -d $BLOCKDB_DATA_DIR collection create force_test
  blockdb-cli -d $BLOCKDB_DATA_DIR collection put col_<force_test_id> "force:3" "test data 3"

  Step 6.2: Test Force Global Flush

  # Test force flush (no confirmation)
  blockdb-cli -d $BLOCKDB_DATA_DIR flush --force
  # Expected: Flushing all database data...
  #           ✅ Database flushed successfully

  # Verify data is gone
  blockdb-cli -d $BLOCKDB_DATA_DIR get "force:1"
  # Expected: Key not found

  blockdb-cli -d $BLOCKDB_DATA_DIR collection list
  # Expected: No collections found.

  Step 6.3: Test Force Collection Flush

  # Recreate test data
  blockdb-cli -d $BLOCKDB_DATA_DIR collection create force_test1
  blockdb-cli -d $BLOCKDB_DATA_DIR collection create force_test2
  blockdb-cli -d $BLOCKDB_DATA_DIR collection put col_<force_test1_id> "test:1" "data 1"
  blockdb-cli -d $BLOCKDB_DATA_DIR collection put col_<force_test2_id> "test:2" "data 2"

  # Test force collection flush
  blockdb-cli -d $BLOCKDB_DATA_DIR collection flush col_<force_test1_id> --force
  # Expected: ✅ Collection 'col_xxx' flushed successfully

  # Verify selective flush worked
  blockdb-cli -d $BLOCKDB_DATA_DIR collection get col_<force_test1_id> "test:1"
  # Expected: Document not found in collection

  blockdb-cli -d $BLOCKDB_DATA_DIR collection get col_<force_test2_id> "test:2"
  # Expected: data 2

  ---
  7. Error Handling & Edge Cases

  Step 7.1: Test Invalid Collection IDs

  # Test flush with non-existent collection
  blockdb-cli -d $BLOCKDB_DATA_DIR collection flush col_nonexistent
  # Expected: Error message about collection not found

  # Test get from non-existent collection
  blockdb-cli -d $BLOCKDB_DATA_DIR collection get col_nonexistent "test:1"
  # Expected: Error message about collection not found

  # Test stats for non-existent collection
  blockdb-cli -d $BLOCKDB_DATA_DIR collection stats col_nonexistent
  # Expected: Error message about collection not found

  Step 7.2: Test Binary Data Handling

  # Test with base64 encoded data
  blockdb-cli -d $BLOCKDB_DATA_DIR put "YmluYXJ5X2tleQ==" "YmluYXJ5X3ZhbHVl" --base64
  blockdb-cli -d $BLOCKDB_DATA_DIR get "YmluYXJ5X2tleQ==" --base64
  # Expected: YmluYXJ5X3ZhbHVl

  # Test flush with binary data
  blockdb-cli -d $BLOCKDB_DATA_DIR flush --force
  blockdb-cli -d $BLOCKDB_DATA_DIR get "YmluYXJ5X2tleQ==" --base64
  # Expected: Key not found

  Step 7.3: Test Append-Only Behavior

  # Add data
  blockdb-cli -d $BLOCKDB_DATA_DIR put "append:1" "original value"
  blockdb-cli -d $BLOCKDB_DATA_DIR get "append:1"
  # Expected: original value

  # Try to update (should fail)
  blockdb-cli -d $BLOCKDB_DATA_DIR put "append:1" "updated value"
  # Expected: Error about duplicate key

  # Verify original value preserved
  blockdb-cli -d $BLOCKDB_DATA_DIR get "append:1"
  # Expected: original value

  # Test flush and re-add same key
  blockdb-cli -d $BLOCKDB_DATA_DIR flush --force
  blockdb-cli -d $BLOCKDB_DATA_DIR put "append:1" "new value after flush"
  blockdb-cli -d $BLOCKDB_DATA_DIR get "append:1"
  # Expected: new value after flush

  ---
  8. Performance and Stress Testing

  Step 8.1: Large Dataset Test

  # Add multiple records
  for i in {1..100}; do
      blockdb-cli -d $BLOCKDB_DATA_DIR put "bulk:$i" "bulk data $i"
  done

  # Verify some records exist
  blockdb-cli -d $BLOCKDB_DATA_DIR get "bulk:1"
  blockdb-cli -d $BLOCKDB_DATA_DIR get "bulk:50"
  blockdb-cli -d $BLOCKDB_DATA_DIR get "bulk:100"

  # Test flush performance
  time blockdb-cli -d $BLOCKDB_DATA_DIR flush --force
  # Expected: Should complete quickly

  # Verify all data is gone
  blockdb-cli -d $BLOCKDB_DATA_DIR get "bulk:1"
  blockdb-cli -d $BLOCKDB_DATA_DIR get "bulk:50"
  blockdb-cli -d $BLOCKDB_DATA_DIR get "bulk:100"
  # Expected: All should return "Key not found"

  Step 8.2: Multiple Collection Performance

  # Create multiple collections with data
  for i in {1..10}; do
      blockdb-cli -d $BLOCKDB_DATA_DIR collection create "collection_$i"
  done

  # Add data to each collection
  # (Replace col_xxx with actual collection IDs)
  for i in {1..10}; do
      for j in {1..10}; do
          blockdb-cli -d $BLOCKDB_DATA_DIR collection put col_<collection_i_id> "key:$j" "data $i-$j"
      done
  done

  # Test collection list performance
  time blockdb-cli -d $BLOCKDB_DATA_DIR collection list

  # Test flush all collections
  time blockdb-cli -d $BLOCKDB_DATA_DIR flush --force

  # Verify all collections are gone
  blockdb-cli -d $BLOCKDB_DATA_DIR collection list
  # Expected: No collections found.

  ---
  9. Recovery and Integrity Testing

  Step 9.1: Test Recovery After Flush

  # Add data and verify integrity
  blockdb-cli -d $BLOCKDB_DATA_DIR put "recovery:1" "test data"
  blockdb-cli -d $BLOCKDB_DATA_DIR verify
  # Expected: ✓ Blockchain integrity verified successfully

  # Flush and verify integrity
  blockdb-cli -d $BLOCKDB_DATA_DIR flush --force
  blockdb-cli -d $BLOCKDB_DATA_DIR verify
  # Expected: ✓ Blockchain integrity verified successfully

  # Add data after flush and verify
  blockdb-cli -d $BLOCKDB_DATA_DIR put "recovery:2" "post-flush data"
  blockdb-cli -d $BLOCKDB_DATA_DIR verify
  # Expected: ✓ Blockchain integrity verified successfully

  Step 9.2: Test Database Restart After Flush

  # Add data
  blockdb-cli -d $BLOCKDB_DATA_DIR put "restart:1" "data before restart"
  blockdb-cli -d $BLOCKDB_DATA_DIR collection create restart_test
  blockdb-cli -d $BLOCKDB_DATA_DIR collection put col_<restart_test_id> "restart:2" "collection data"

  # Verify data exists
  blockdb-cli -d $BLOCKDB_DATA_DIR get "restart:1"
  blockdb-cli -d $BLOCKDB_DATA_DIR collection get col_<restart_test_id> "restart:2"

  # Flush
  blockdb-cli -d $BLOCKDB_DATA_DIR flush --force

  # Restart would happen here in a real scenario
  # For testing, just verify the database works after flush
  blockdb-cli -d $BLOCKDB_DATA_DIR stats
  blockdb-cli -d $BLOCKDB_DATA_DIR verify

  # Add new data to verify database is functional
  blockdb-cli -d $BLOCKDB_DATA_DIR put "restart:3" "data after restart"
  blockdb-cli -d $BLOCKDB_DATA_DIR get "restart:3"
  # Expected: data after restart

  ---
  10. Final Cleanup

  # Clean up test data
  rm -rf /tmp/blockdb-test

  # Unset environment variables
  unset BLOCKDB_DATA_DIR
  unalias blockdb-cli

  ---
  Expected Results Summary

  ✅ Global Flush Results

  - All database data cleared
  - Collections removed
  - WAL reset
  - Blockchain reset to genesis
  - Database remains functional

  ✅ Collection Flush Results

  - Specific collection data cleared
  - Collection statistics reset
  - Other collections unaffected
  - Collection metadata preserved

  ✅ Safety Features

  - Confirmation prompts work
  - Force flags bypass confirmations
  - Clear warning messages
  - Graceful error handling

  ✅ Performance

  - Fast flush operations
  - Efficient memory cleanup
  - Quick recovery after flush
  - Minimal system impact

  ✅ Integrity

  - Blockchain verification passes
  - Database remains consistent
  - Proper error handling
  - Safe state transitions

  This comprehensive test plan validates all aspects of the flush functionality while ensuring data safety and system integrity.