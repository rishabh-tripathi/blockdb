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

impl BlockDB {
    pub fn new(config: BlockDBConfig) -> Result<Self, Box<dyn std::error::Error>> {
        std::fs::create_dir_all(&config.data_dir)?;
        
        let memtable = Arc::new(RwLock::new(memtable::MemTable::new()));
        let wal = Arc::new(Mutex::new(wal::WriteAheadLog::new(&config.data_dir)?));
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
        
        // Recover from WAL on startup
        db.recover_from_wal()?;
        
        Ok(db)
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
            *counter = max_sequence;
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

        {
            let mut wal = self.wal.lock().unwrap();
            wal.append(&record)?;
        }

        {
            let mut memtable = self.memtable.write().unwrap();
            memtable.insert(record.clone());
            
            if memtable.size() > self.config.memtable_size_limit {
                drop(memtable);
                self.flush_memtable()?;
            }
        }

        {
            let mut blockchain = self.blockchain.lock().unwrap();
            blockchain.add_record(record)?;
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

    fn flush_memtable(&self) -> Result<(), Box<dyn std::error::Error>> {
        let memtable = {
            let mut memtable_guard = self.memtable.write().unwrap();
            let old_memtable = std::mem::replace(&mut *memtable_guard, memtable::MemTable::new());
            old_memtable
        };

        let sstable_path = format!("{}/sstable_{}.sst", 
            self.config.data_dir, 
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        );

        let sstable = sstable::SSTable::create_from_memtable(&sstable_path, &memtable)?;
        
        {
            let mut sstables = self.sstables.write().unwrap();
            sstables.push(sstable);
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