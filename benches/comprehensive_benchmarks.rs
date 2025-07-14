use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use blockdb::{BlockDBHandle, BlockDBConfig, AuthManager, Permission};
use blockdb::api::{BlockDBServer, ApiConfig, WriteRequest, ReadRequest, LoginRequest};
use tempfile::TempDir;
use std::sync::Arc;
use tokio::runtime::Runtime;

/// Comprehensive performance benchmarks for BlockDB
/// Tests throughput, latency, and scalability under various conditions

fn create_test_db() -> (BlockDBHandle, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        memtable_size_limit: 64 * 1024 * 1024, // 64MB
        wal_sync_interval_ms: 1000,
        compaction_threshold: 4,
        blockchain_batch_size: 1000,
        auth_enabled: false, // Disable auth for performance tests unless specifically testing auth
        ..Default::default()
    };
    
    let db = BlockDBHandle::new(config).unwrap();
    (db, temp_dir)
}

fn bench_sequential_writes(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential_writes");
    group.throughput(Throughput::Elements(1000));

    let rt = Runtime::new().unwrap();
    
    for size in [100, 1000, 5000].iter() {
        group.bench_with_input(BenchmarkId::new("write_ops", size), size, |b, &size| {
            b.iter(|| {
                rt.block_on(async {
                    let (db, _temp_dir) = create_test_db();
                    
                    for i in 0..size {
                        let key = format!("bench_key_{}", i);
                        let value = format!("bench_value_{}_{'x'.repeat(100)}", i); // ~120 bytes per value
                        
                        db.put(key.as_bytes(), value.as_bytes()).await.unwrap();
                    }
                    
                    black_box(db);
                })
            })
        });
    }
    group.finish();
}

fn bench_random_reads(c: &mut Criterion) {
    let mut group = c.benchmark_group("random_reads");
    group.throughput(Throughput::Elements(1000));

    let rt = Runtime::new().unwrap();
    
    // Pre-populate database
    let (db, _temp_dir) = create_test_db();
    rt.block_on(async {
        for i in 0..10000 {
            let key = format!("read_key_{}", i);
            let value = format!("read_value_{}", i);
            db.put(key.as_bytes(), value.as_bytes()).await.unwrap();
        }
        db.force_flush().await.unwrap();
    });
    
    let db = Arc::new(db);

    for pattern in ["sequential", "random"].iter() {
        group.bench_with_input(BenchmarkId::new("read_pattern", pattern), pattern, |b, &pattern| {
            let db_clone = db.clone();
            b.iter(|| {
                rt.block_on(async {
                    for i in 0..1000 {
                        let key_id = if pattern == "sequential" {
                            i
                        } else {
                            fastrand::usize(0..10000)
                        };
                        
                        let key = format!("read_key_{}", key_id);
                        let result = db_clone.get(key.as_bytes()).await.unwrap();
                        black_box(result);
                    }
                })
            })
        });
    }
    group.finish();
}

fn bench_mixed_workload(c: &mut Criterion) {
    let mut group = c.benchmark_group("mixed_workload");
    group.throughput(Throughput::Elements(1000));

    let rt = Runtime::new().unwrap();

    for read_write_ratio in [(90, 10), (70, 30), (50, 50)].iter() {
        let (read_pct, write_pct) = read_write_ratio;
        group.bench_with_input(
            BenchmarkId::new("read_write_ratio", format!("{}r_{}w", read_pct, write_pct)),
            read_write_ratio,
            |b, &(read_pct, _write_pct)| {
                b.iter(|| {
                    rt.block_on(async {
                        let (db, _temp_dir) = create_test_db();
                        let mut write_counter = 0;
                        
                        // Pre-populate for reads
                        for i in 0..1000 {
                            let key = format!("mixed_key_{}", i);
                            let value = format!("mixed_value_{}", i);
                            db.put(key.as_bytes(), value.as_bytes()).await.unwrap();
                        }
                        
                        // Mixed operations
                        for i in 0..1000 {
                            if fastrand::usize(0..100) < read_pct {
                                // Read operation
                                let key_id = fastrand::usize(0..1000);
                                let key = format!("mixed_key_{}", key_id);
                                let result = db.get(key.as_bytes()).await.unwrap();
                                black_box(result);
                            } else {
                                // Write operation (new keys to avoid conflicts)
                                let key = format!("mixed_new_key_{}", write_counter);
                                let value = format!("mixed_new_value_{}", write_counter);
                                db.put(key.as_bytes(), value.as_bytes()).await.unwrap();
                                write_counter += 1;
                            }
                        }
                        
                        black_box(db);
                    })
                })
            }
        );
    }
    group.finish();
}

fn bench_data_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("data_sizes");
    
    let rt = Runtime::new().unwrap();

    for size_kb in [1, 10, 100, 1000].iter() {
        let size_bytes = size_kb * 1024;
        group.throughput(Throughput::Bytes(size_bytes as u64));
        
        group.bench_with_input(
            BenchmarkId::new("value_size_kb", size_kb),
            &size_bytes,
            |b, &size_bytes| {
                b.iter(|| {
                    rt.block_on(async {
                        let (db, _temp_dir) = create_test_db();
                        
                        let key = b"large_value_key";
                        let value = vec![b'x'; size_bytes];
                        
                        db.put(key, &value).await.unwrap();
                        let retrieved = db.get(key).await.unwrap();
                        
                        black_box(retrieved);
                    })
                })
            }
        );
    }
    group.finish();
}

fn bench_concurrent_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_operations");
    group.throughput(Throughput::Elements(1000));

    let rt = Runtime::new().unwrap();

    for num_tasks in [1, 2, 4, 8, 16].iter() {
        group.bench_with_input(
            BenchmarkId::new("concurrent_tasks", num_tasks),
            num_tasks,
            |b, &num_tasks| {
                b.iter(|| {
                    rt.block_on(async {
                        let (db, _temp_dir) = create_test_db();
                        let db = Arc::new(db);
                        let operations_per_task = 1000 / num_tasks;
                        
                        let mut handles = Vec::new();
                        
                        for task_id in 0..num_tasks {
                            let db_clone = db.clone();
                            let handle = tokio::spawn(async move {
                                for i in 0..operations_per_task {
                                    let key = format!("concurrent_key_{}_{}", task_id, i);
                                    let value = format!("concurrent_value_{}_{}", task_id, i);
                                    
                                    db_clone.put(key.as_bytes(), value.as_bytes()).await.unwrap();
                                }
                            });
                            handles.push(handle);
                        }
                        
                        for handle in handles {
                            handle.await.unwrap();
                        }
                        
                        black_box(db);
                    })
                })
            }
        );
    }
    group.finish();
}

fn bench_authentication_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("authentication");
    
    let rt = Runtime::new().unwrap();

    group.bench_function("user_creation", |b| {
        b.iter(|| {
            rt.block_on(async {
                let temp_dir = TempDir::new().unwrap();
                let config = BlockDBConfig {
                    data_dir: temp_dir.path().to_string_lossy().to_string(),
                    auth_enabled: true,
                    ..Default::default()
                };
                
                let mut auth_manager = AuthManager::new(config).unwrap();
                
                for i in 0..100 {
                    let username = format!("bench_user_{}", i);
                    let password = format!("bench_pass_{}", i);
                    let permissions = vec![Permission::Read, Permission::Write];
                    
                    auth_manager.create_user(&username, &password, permissions).unwrap();
                }
                
                black_box(auth_manager);
            })
        })
    });

    group.bench_function("user_authentication", |b| {
        b.iter(|| {
            rt.block_on(async {
                let temp_dir = TempDir::new().unwrap();
                let config = BlockDBConfig {
                    data_dir: temp_dir.path().to_string_lossy().to_string(),
                    auth_enabled: true,
                    ..Default::default()
                };
                
                let mut auth_manager = AuthManager::new(config).unwrap();
                
                // Pre-create users
                for i in 0..100 {
                    let username = format!("auth_user_{}", i);
                    let password = format!("auth_pass_{}", i);
                    auth_manager.create_user(&username, &password, vec![Permission::Read]).unwrap();
                }
                
                // Benchmark authentication
                for i in 0..100 {
                    let username = format!("auth_user_{}", i);
                    let password = format!("auth_pass_{}", i);
                    let context = auth_manager.authenticate_user(&username, &password).unwrap();
                    black_box(context);
                }
            })
        })
    });

    group.bench_function("session_validation", |b| {
        b.iter(|| {
            rt.block_on(async {
                let temp_dir = TempDir::new().unwrap();
                let config = BlockDBConfig {
                    data_dir: temp_dir.path().to_string_lossy().to_string(),
                    auth_enabled: true,
                    ..Default::default()
                };
                
                let mut auth_manager = AuthManager::new(config).unwrap();
                
                // Create user and get session
                auth_manager.create_user("session_user", "session_pass", vec![Permission::Read]).unwrap();
                let context = auth_manager.authenticate_user("session_user", "session_pass").unwrap();
                let session_id = context.session_id.clone();
                
                // Benchmark session validation
                for _ in 0..1000 {
                    let validated = auth_manager.validate_session(&session_id).unwrap();
                    black_box(validated);
                }
            })
        })
    });

    group.finish();
}

fn bench_api_layer_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("api_layer");
    group.throughput(Throughput::Elements(1000));
    
    let rt = Runtime::new().unwrap();

    group.bench_function("authenticated_api_writes", |b| {
        b.iter(|| {
            rt.block_on(async {
                let temp_dir = TempDir::new().unwrap();
                let config = BlockDBConfig {
                    data_dir: temp_dir.path().to_string_lossy().to_string(),
                    auth_enabled: true,
                    ..Default::default()
                };

                let db = BlockDBHandle::new(config.clone()).unwrap();
                let mut auth_manager = AuthManager::new(config).unwrap();
                
                // Create user
                auth_manager.create_user("api_user", "api_pass", vec![Permission::Write]).unwrap();
                let context = auth_manager.authenticate_user("api_user", "api_pass").unwrap();
                
                let api_config = ApiConfig {
                    auth_enabled: true,
                    ..Default::default()
                };
                let server = BlockDBServer::with_auth(db, api_config, auth_manager);
                
                // Benchmark authenticated writes
                for i in 0..100 {
                    let request = WriteRequest {
                        key: format!("api_key_{}", i),
                        value: format!("api_value_{}", i),
                        encoding: None,
                        auth_token: Some(context.session_id.clone()),
                    };
                    
                    let response = server.write(request).await.unwrap();
                    black_box(response);
                }
            })
        })
    });

    group.bench_function("unauthenticated_api_writes", |b| {
        b.iter(|| {
            rt.block_on(async {
                let temp_dir = TempDir::new().unwrap();
                let config = BlockDBConfig {
                    data_dir: temp_dir.path().to_string_lossy().to_string(),
                    auth_enabled: false,
                    ..Default::default()
                };

                let db = BlockDBHandle::new(config).unwrap();
                let api_config = ApiConfig {
                    auth_enabled: false,
                    ..Default::default()
                };
                let server = BlockDBServer::new(db, api_config);
                
                // Benchmark unauthenticated writes
                for i in 0..100 {
                    let request = WriteRequest {
                        key: format!("unauth_key_{}", i),
                        value: format!("unauth_value_{}", i),
                        encoding: None,
                        auth_token: None,
                    };
                    
                    let response = server.write(request).await.unwrap();
                    black_box(response);
                }
            })
        })
    });

    group.finish();
}

fn bench_blockchain_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("blockchain");
    
    let rt = Runtime::new().unwrap();

    group.bench_function("blockchain_verification", |b| {
        b.iter(|| {
            rt.block_on(async {
                let (db, _temp_dir) = create_test_db();
                
                // Add data to create blockchain entries
                for i in 0..1000 {
                    let key = format!("blockchain_key_{}", i);
                    let value = format!("blockchain_value_{}", i);
                    db.put(key.as_bytes(), value.as_bytes()).await.unwrap();
                }
                
                // Benchmark verification
                let is_valid = db.verify_integrity().await.unwrap();
                black_box(is_valid);
            })
        })
    });

    for batch_size in [100, 500, 1000, 2000].iter() {
        group.bench_with_input(
            BenchmarkId::new("blockchain_batch_size", batch_size),
            batch_size,
            |b, &batch_size| {
                b.iter(|| {
                    rt.block_on(async {
                        let temp_dir = TempDir::new().unwrap();
                        let config = BlockDBConfig {
                            data_dir: temp_dir.path().to_string_lossy().to_string(),
                            blockchain_batch_size: batch_size,
                            ..Default::default()
                        };
                        
                        let db = BlockDBHandle::new(config).unwrap();
                        
                        // Write operations to trigger blockchain batching
                        for i in 0..batch_size * 2 {
                            let key = format!("batch_key_{}", i);
                            let value = format!("batch_value_{}", i);
                            db.put(key.as_bytes(), value.as_bytes()).await.unwrap();
                        }
                        
                        db.force_flush().await.unwrap();
                        black_box(db);
                    })
                })
            }
        );
    }

    group.finish();
}

fn bench_memory_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_usage");
    
    let rt = Runtime::new().unwrap();

    for memtable_size_mb in [1, 4, 16, 64].iter() {
        let memtable_size = memtable_size_mb * 1024 * 1024;
        group.bench_with_input(
            BenchmarkId::new("memtable_size_mb", memtable_size_mb),
            &memtable_size,
            |b, &memtable_size| {
                b.iter(|| {
                    rt.block_on(async {
                        let temp_dir = TempDir::new().unwrap();
                        let config = BlockDBConfig {
                            data_dir: temp_dir.path().to_string_lossy().to_string(),
                            memtable_size_limit: memtable_size,
                            compaction_threshold: 2,
                            ..Default::default()
                        };
                        
                        let db = BlockDBHandle::new(config).unwrap();
                        
                        // Fill memtable and trigger flushes
                        let value_size = 1000; // 1KB values
                        let num_operations = (memtable_size * 3) / value_size; // 3x memtable size
                        
                        for i in 0..num_operations {
                            let key = format!("memory_key_{}", i);
                            let value = vec![b'x'; value_size];
                            db.put(key.as_bytes(), &value).await.unwrap();
                        }
                        
                        db.force_flush().await.unwrap();
                        black_box(db);
                    })
                })
            }
        );
    }

    group.finish();
}

fn bench_flush_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("flush_operations");
    
    let rt = Runtime::new().unwrap();

    for data_size in [1000, 5000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::new("flush_after_writes", data_size),
            data_size,
            |b, &data_size| {
                b.iter(|| {
                    rt.block_on(async {
                        let (db, _temp_dir) = create_test_db();
                        
                        // Write data
                        for i in 0..data_size {
                            let key = format!("flush_key_{}", i);
                            let value = format!("flush_value_{}", i);
                            db.put(key.as_bytes(), value.as_bytes()).await.unwrap();
                        }
                        
                        // Benchmark flush operation
                        db.flush_all().await.unwrap();
                        black_box(db);
                    })
                })
            }
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_sequential_writes,
    bench_random_reads,
    bench_mixed_workload,
    bench_data_sizes,
    bench_concurrent_operations,
    bench_authentication_operations,
    bench_api_layer_performance,
    bench_blockchain_operations,
    bench_memory_usage,
    bench_flush_operations
);

criterion_main!(benches);