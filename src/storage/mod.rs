use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, RwLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use sha2::{Sha256, Digest};
use serde::{Serialize, Deserialize};

pub mod wal;
pub mod memtable;
pub mod sstable;
pub mod blockchain;
pub mod compaction;
pub mod collection;
pub mod format;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub timestamp: u64,
    pub sequence_number: u64,
    pub hash: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct BlockDBConfig {
    pub data_dir: String,
    pub memtable_size_limit: usize,
    pub wal_sync_interval_ms: u64,
    pub compaction_threshold: usize,
    pub blockchain_batch_size: usize,
    pub auth_enabled: bool,
    pub session_duration_hours: u64,
    pub password_min_length: usize,
    pub max_failed_attempts: u32,
    pub account_lockout_duration_minutes: u64,
}

impl Default for BlockDBConfig {
    fn default() -> Self {
        Self {
            data_dir: "./blockdb_data".to_string(),
            memtable_size_limit: 64 * 1024 * 1024, // 64MB
            wal_sync_interval_ms: 1000, // 1 second
            compaction_threshold: 4,
            blockchain_batch_size: 1000,
            auth_enabled: true,
            session_duration_hours: 24,
            password_min_length: 8,
            max_failed_attempts: 5,
            account_lockout_duration_minutes: 30,
        }
    }
}

pub struct BlockDB {
    config: BlockDBConfig,
    memtable: Arc<RwLock<memtable::MemTable>>,
    wal: Arc<Mutex<wal::WriteAheadLog>>,
    sstables: Arc<RwLock<Vec<sstable::SSTable>>>,
    blockchain: Arc<Mutex<blockchain::BlockChain>>,
    sequence_counter: Arc<Mutex<u64>>,
}

impl std::fmt::Debug for BlockDB {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockDB")
            .field("data_dir", &self.config.data_dir)
            .field("sequence_counter", &*self.sequence_counter.lock().unwrap())
            .finish()
    }
}

impl BlockDB {
    pub fn new(config: BlockDBConfig) -> Result<Self, Box<dyn std::error::Error>> {
        std::fs::create_dir_all(&config.data_dir)?;

        // Verify (and stamp) the on-disk format version before touching any data
        // files, so a directory written by a newer build is rejected up front.
        format::open_or_init(&config.data_dir)?;

        // Durability model driven by wal_sync_interval_ms:
        //   0  -> fsync on every append (strongest; acknowledged writes survive power loss)
        //   >0 -> flush to the OS on every append and fsync at memtable-flush boundaries
        //         (higher throughput; a crash may lose the most recent un-flushed writes).
        let sync_mode = if config.wal_sync_interval_ms == 0 {
            wal::SyncMode::Always
        } else {
            wal::SyncMode::Periodic
        };

        let memtable = Arc::new(RwLock::new(memtable::MemTable::new()));
        let wal = Arc::new(Mutex::new(wal::WriteAheadLog::with_sync_mode(
            &config.data_dir,
            sync_mode,
        )?));
        let sstables = Arc::new(RwLock::new(Vec::new()));
        let blockchain = Arc::new(Mutex::new(blockchain::BlockChain::new(&config.data_dir)?));
        let sequence_counter = Arc::new(Mutex::new(0));

        let db = BlockDB {
            config,
            memtable,
            wal,
            sstables,
            blockchain,
            sequence_counter,
        };

        // Load previously-flushed SSTables from disk, then replay the WAL for the
        // records that had not yet been flushed at shutdown/crash time.
        db.load_sstables()?;
        db.recover_from_wal()?;

        Ok(db)
    }

    /// Discover and open the SSTable files already present in the data directory.
    /// Files are named with a monotonically increasing nanosecond timestamp, so a
    /// lexical sort by filename restores their original write order (oldest first).
    fn load_sstables(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut sstable_files: Vec<std::path::PathBuf> = std::fs::read_dir(&self.config.data_dir)?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.extension().and_then(|s| s.to_str()) == Some("sst"))
            .collect();
        sstable_files.sort();

        let mut sstables = self.sstables.write().unwrap();
        let mut max_sequence = 0u64;
        for path in sstable_files {
            let path_str = path.to_string_lossy().to_string();
            match sstable::SSTable::open(&path_str) {
                Ok(mut table) => {
                    // Track the highest sequence number so new writes continue the sequence.
                    let keys: Vec<Vec<u8>> = table.iter().cloned().collect();
                    for key in keys {
                        if let Ok(Some(record)) = table.get(&key) {
                            max_sequence = max_sequence.max(record.sequence_number);
                        }
                    }
                    sstables.push(table);
                }
                Err(e) => {
                    eprintln!("Warning: skipping unreadable SSTable {}: {}", path_str, e);
                }
            }
        }

        if max_sequence > 0 {
            let mut counter = self.sequence_counter.lock().unwrap();
            *counter = (*counter).max(max_sequence);
        }

        Ok(())
    }

    fn recover_from_wal(&self) -> Result<(), Box<dyn std::error::Error>> {
        let wal = self.wal.lock().unwrap();
        let records = wal.recover()?;

        if !records.is_empty() {
            let mut memtable = self.memtable.write().unwrap();
            let mut max_sequence = 0u64;

            for record in records {
                max_sequence = max_sequence.max(record.sequence_number);
                memtable.insert(record);
            }

            // Update sequence counter
            let mut counter = self.sequence_counter.lock().unwrap();
            *counter = (*counter).max(max_sequence);
        }

        Ok(())
    }

    fn key_exists(&self, key: &[u8]) -> Result<bool, Box<dyn std::error::Error>> {
        // Check memtable first
        {
            let memtable = self.memtable.read().unwrap();
            if memtable.get(key).is_some() {
                return Ok(true);
            }
        }

        // Check SSTables
        {
            let mut sstables = self.sstables.write().unwrap();
            for sstable in sstables.iter_mut().rev() {
                if sstable.get(key)?.is_some() {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        // Check if key already exists (append-only database)
        if self.key_exists(key)? {
            return Err(Box::new(crate::error::BlockDBError::DuplicateKey(
                format!("Key '{}' already exists. BlockDB is append-only and does not allow updates.", 
                    String::from_utf8_lossy(key))
            )));
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let sequence_number = {
            let mut counter = self.sequence_counter.lock().unwrap();
            *counter += 1;
            *counter
        };

        let mut hasher = Sha256::new();
        hasher.update(key);
        hasher.update(value);
        hasher.update(&timestamp.to_be_bytes());
        hasher.update(&sequence_number.to_be_bytes());
        let hash = hasher.finalize().to_vec();

        let record = Record {
            key: key.to_vec(),
            value: value.to_vec(),
            timestamp,
            sequence_number,
            hash,
        };

        // 1. Record the write intent in the WAL first (write-ahead durability).
        {
            let mut wal = self.wal.lock().unwrap();
            wal.append(&record)?;
        }

        // 2. Add to the blockchain's pending set *before* any flush so that if the
        //    memtable flush seals a block, this record is included in the audit trail.
        {
            let mut blockchain = self.blockchain.lock().unwrap();
            blockchain.add_record(record.clone())?;
        }

        // 3. Insert into the memtable and flush if it has grown past the limit.
        let needs_flush = {
            let mut memtable = self.memtable.write().unwrap();
            memtable.insert(record);
            memtable.size() > self.config.memtable_size_limit
        };
        if needs_flush {
            self.flush_memtable()?;
        }

        Ok(())
    }

    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
        {
            let memtable = self.memtable.read().unwrap();
            if let Some(record) = memtable.get(key) {
                return Ok(Some(record.value.clone()));
            }
        }

        {
            let mut sstables = self.sstables.write().unwrap();
            for sstable in sstables.iter_mut().rev() {
                if let Some(record) = sstable.get(key)? {
                    return Ok(Some(record.value));
                }
            }
        }

        Ok(None)
    }

    /// Return every key/value pair whose key starts with `prefix`, gathered across
    /// the memtable and all SSTables, sorted ascending by key. Used by higher layers
    /// (e.g. the consent event log) that need range scans over an append-only keyspace.
    pub fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Box<dyn std::error::Error>> {
        let mut merged: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();

        // SSTables first (oldest to newest) so newer data overrides on key collisions.
        {
            let mut sstables = self.sstables.write().unwrap();
            for sstable in sstables.iter_mut() {
                let keys: Vec<Vec<u8>> = sstable
                    .iter()
                    .filter(|k| k.starts_with(prefix))
                    .cloned()
                    .collect();
                for key in keys {
                    if let Some(record) = sstable.get(&key)? {
                        merged.insert(key, record.value);
                    }
                }
            }
        }

        // Memtable last so the freshest values win.
        {
            let memtable = self.memtable.read().unwrap();
            for (key, record) in memtable.iter() {
                if key.starts_with(prefix) {
                    merged.insert(key.clone(), record.value.clone());
                }
            }
        }

        Ok(merged.into_iter().collect())
    }

    fn flush_memtable(&self) -> Result<(), Box<dyn std::error::Error>> {
        let memtable = {
            let mut memtable_guard = self.memtable.write().unwrap();
            let old_memtable = std::mem::replace(&mut *memtable_guard, memtable::MemTable::new());
            old_memtable
        };

        if memtable.is_empty() {
            return Ok(());
        }

        let sstable_path = format!("{}/sstable_{}.sst",
            self.config.data_dir,
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        );

        // 1. Write and fsync the SSTable so the flushed records are durable on disk.
        let sstable = sstable::SSTable::create_from_memtable(&sstable_path, &memtable)?;

        {
            let mut sstables = self.sstables.write().unwrap();
            sstables.push(sstable);
        }

        // 2. Seal any pending blockchain records so the immutable audit trail is
        //    persisted before we drop the WAL that currently backs them.
        {
            let mut blockchain = self.blockchain.lock().unwrap();
            blockchain.force_create_block()?;
        }

        // 3. The records are now durable in both the SSTable and the blockchain, so
        //    the WAL that protected them can be truncated to keep it bounded.
        {
            let mut wal = self.wal.lock().unwrap();
            wal.truncate()?;
        }

        // 4. Merge SSTables once we exceed the configured threshold.
        self.maybe_compact()?;

        Ok(())
    }

    /// Size-tiered compaction: when the number of on-disk SSTables exceeds the
    /// configured threshold, merge them all into a single compacted SSTable and
    /// delete the sources. BlockDB is append-only, so a merge is a straight union
    /// of keys with no conflict resolution required.
    fn maybe_compact(&self) -> Result<(), Box<dyn std::error::Error>> {
        let should_compact = {
            let sstables = self.sstables.read().unwrap();
            sstables.len() > self.config.compaction_threshold
        };
        if !should_compact {
            return Ok(());
        }

        // Collect every record across all current SSTables (dedup by key via BTreeMap).
        let (merged, old_paths): (BTreeMap<Vec<u8>, Record>, Vec<String>) = {
            let mut sstables = self.sstables.write().unwrap();
            let mut merged = BTreeMap::new();
            let mut old_paths = Vec::new();
            for table in sstables.iter_mut() {
                let keys: Vec<Vec<u8>> = table.iter().cloned().collect();
                for key in keys {
                    if let Some(record) = table.get(&key)? {
                        merged.insert(key, record);
                    }
                }
                old_paths.push(table.path().to_string());
            }
            (merged, old_paths)
        };

        if merged.is_empty() {
            return Ok(());
        }

        // Materialize the merged records into a temporary memtable and write one SSTable.
        let mut merged_memtable = memtable::MemTable::new();
        for (_key, record) in merged {
            merged_memtable.insert(record);
        }
        let compacted_path = format!("{}/compacted_{}.sst",
            self.config.data_dir,
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        );
        let compacted = sstable::SSTable::create_from_memtable(&compacted_path, &merged_memtable)?;

        // Swap the live set to just the compacted table, then remove the old files.
        {
            let mut sstables = self.sstables.write().unwrap();
            sstables.clear();
            sstables.push(compacted);
        }
        for path in old_paths {
            let _ = std::fs::remove_file(path);
        }

        Ok(())
    }

    pub fn verify_integrity(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let blockchain = self.blockchain.lock().unwrap();
        blockchain.verify_chain()
    }

    /// Flush all data and reset the database to an empty state
    pub fn flush_all(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Clear memtable
        {
            let mut memtable = self.memtable.write().unwrap();
            *memtable = memtable::MemTable::new();
        }

        // Clear WAL
        {
            let mut wal = self.wal.lock().unwrap();
            wal.clear()?;
        }

        // Clear SSTables
        {
            let mut sstables = self.sstables.write().unwrap();
            // Remove all SSTable files
            for sstable in sstables.drain(..) {
                let _ = std::fs::remove_file(sstable.path());
            }
        }

        // Clear blockchain
        {
            let mut blockchain = self.blockchain.lock().unwrap();
            blockchain.clear()?;
        }

        // Reset sequence counter
        {
            let mut counter = self.sequence_counter.lock().unwrap();
            *counter = 0;
        }

        println!("✅ Database flushed successfully - all data cleared");
        Ok(())
    }

    /// Force flush memtable to disk
    pub fn force_flush_memtable(&self) -> Result<(), Box<dyn std::error::Error>> {
        let memtable = self.memtable.read().unwrap();
        if !memtable.is_empty() {
            drop(memtable);
            self.flush_memtable()?;
            println!("✅ Memtable flushed to disk");
        } else {
            println!("ℹ️ Memtable is empty, nothing to flush");
        }
        Ok(())
    }
}