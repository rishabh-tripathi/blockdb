use blockdb::{BlockDBConfig, BlockDBHandle};
use std::time::Instant;
use tempfile::TempDir;

#[tokio::test]
async fn test_write_performance() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        memtable_size_limit: 10 * 1024 * 1024, // 10MB
        ..Default::default()
    };

    let db = BlockDBHandle::new(config).unwrap();
    
    let num_operations = 1000;
    let start_time = Instant::now();
    
    // Perform writes
    for i in 0..num_operations {
        let key = format!("key_{:06}", i);
        let value = format!("value_{:06}_some_longer_content_to_test_performance", i);
        db.put(key.as_bytes(), value.as_bytes()).await.unwrap();
    }
    
    let write_duration = start_time.elapsed();
    println!("Completed {} writes in {:?}", num_operations, write_duration);
    println!("Write throughput: {:.2} ops/sec", num_operations as f64 / write_duration.as_secs_f64());
    
    // Perform reads
    let start_time = Instant::now();
    
    for i in 0..num_operations {
        let key = format!("key_{:06}", i);
        let result = db.get(key.as_bytes()).await.unwrap();
        assert!(result.is_some());
    }
    
    let read_duration = start_time.elapsed();
    println!("Completed {} reads in {:?}", num_operations, read_duration);
    println!("Read throughput: {:.2} ops/sec", num_operations as f64 / read_duration.as_secs_f64());
    
    // Verify integrity
    let start_time = Instant::now();
    let is_valid = db.verify_integrity().await.unwrap();
    let verify_duration = start_time.elapsed();
    
    assert!(is_valid);
    println!("Blockchain integrity verification completed in {:?}", verify_duration);
}

#[tokio::test]
async fn test_concurrent_writes() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        ..Default::default()
    };

    let db = BlockDBHandle::new(config).unwrap();
    
    let num_tasks = 10;
    let operations_per_task = 100;
    
    let start_time = Instant::now();
    
    let mut handles = Vec::new();
    
    for task_id in 0..num_tasks {
        let db_clone = db.clone();
        let handle = tokio::spawn(async move {
            for i in 0..operations_per_task {
                let key = format!("task_{}_key_{:06}", task_id, i);
                let value = format!("task_{}_value_{:06}", task_id, i);
                db_clone.put(key.as_bytes(), value.as_bytes()).await.unwrap();
            }
        });
        handles.push(handle);
    }
    
    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    let duration = start_time.elapsed();
    let total_operations = num_tasks * operations_per_task;
    
    println!("Completed {} concurrent writes in {:?}", total_operations, duration);
    println!("Concurrent write throughput: {:.2} ops/sec", total_operations as f64 / duration.as_secs_f64());
    
    // Verify all data was written
    for task_id in 0..num_tasks {
        for i in 0..operations_per_task {
            let key = format!("task_{}_key_{:06}", task_id, i);
            let result = db.get(key.as_bytes()).await.unwrap();
            assert!(result.is_some());
        }
    }
    
    println!("All concurrent writes verified successfully");
}