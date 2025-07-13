use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use serde::{Serialize, Deserialize};
use crate::error::BlockDBError;
use crate::transaction::TransactionId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionLogEntry {
    Begin {
        transaction_id: TransactionId,
        timestamp: u64,
    },
    Prepare {
        transaction_id: TransactionId,
        timestamp: u64,
    },
    Commit {
        transaction_id: TransactionId,
        timestamp: u64,
    },
    Abort {
        transaction_id: TransactionId,
        timestamp: u64,
    },
}

impl TransactionLogEntry {
    pub fn transaction_id(&self) -> &TransactionId {
        match self {
            TransactionLogEntry::Begin { transaction_id, .. } => transaction_id,
            TransactionLogEntry::Prepare { transaction_id, .. } => transaction_id,
            TransactionLogEntry::Commit { transaction_id, .. } => transaction_id,
            TransactionLogEntry::Abort { transaction_id, .. } => transaction_id,
        }
    }
    
    pub fn timestamp(&self) -> u64 {
        match self {
            TransactionLogEntry::Begin { timestamp, .. } => *timestamp,
            TransactionLogEntry::Prepare { timestamp, .. } => *timestamp,
            TransactionLogEntry::Commit { timestamp, .. } => *timestamp,
            TransactionLogEntry::Abort { timestamp, .. } => *timestamp,
        }
    }
}

#[derive(Debug)]
pub struct TransactionLog {
    file: BufWriter<File>,
    path: String,
}

impl TransactionLog {
    pub fn new(data_dir: &str) -> Result<Self, BlockDBError> {
        let path = format!("{}/transaction.log", data_dir);
        
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(&path)
            .map_err(|e| BlockDBError::IoError(e))?;
        
        Ok(TransactionLog {
            file: BufWriter::new(file),
            path,
        })
    }
    
    fn current_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }
    
    pub fn log_begin(&mut self, transaction_id: &TransactionId) -> Result<(), BlockDBError> {
        let entry = TransactionLogEntry::Begin {
            transaction_id: transaction_id.clone(),
            timestamp: Self::current_timestamp(),
        };
        self.write_entry(&entry)
    }
    
    pub fn log_prepare(&mut self, transaction_id: &TransactionId) -> Result<(), BlockDBError> {
        let entry = TransactionLogEntry::Prepare {
            transaction_id: transaction_id.clone(),
            timestamp: Self::current_timestamp(),
        };
        self.write_entry(&entry)
    }
    
    pub fn log_commit(&mut self, transaction_id: &TransactionId) -> Result<(), BlockDBError> {
        let entry = TransactionLogEntry::Commit {
            transaction_id: transaction_id.clone(),
            timestamp: Self::current_timestamp(),
        };
        self.write_entry(&entry)
    }
    
    pub fn log_abort(&mut self, transaction_id: &TransactionId) -> Result<(), BlockDBError> {
        let entry = TransactionLogEntry::Abort {
            transaction_id: transaction_id.clone(),
            timestamp: Self::current_timestamp(),
        };
        self.write_entry(&entry)
    }
    
    fn write_entry(&mut self, entry: &TransactionLogEntry) -> Result<(), BlockDBError> {
        let serialized = bincode::serialize(entry)
            .map_err(|e| BlockDBError::SerializationError(e))?;
        
        let size = serialized.len() as u32;
        
        // Write size prefix
        self.file.write_all(&size.to_be_bytes())
            .map_err(|e| BlockDBError::IoError(e))?;
        
        // Write entry
        self.file.write_all(&serialized)
            .map_err(|e| BlockDBError::IoError(e))?;
        
        // Ensure it's written to disk
        self.file.flush()
            .map_err(|e| BlockDBError::IoError(e))?;
        
        Ok(())
    }
    
    pub fn recover(&self) -> Result<Vec<TransactionLogEntry>, BlockDBError> {
        if !Path::new(&self.path).exists() {
            return Ok(Vec::new());
        }
        
        let file = File::open(&self.path)
            .map_err(|e| BlockDBError::IoError(e))?;
        
        let mut reader = BufReader::new(file);
        let mut entries = Vec::new();
        
        loop {
            // Read size prefix
            let mut size_buf = [0u8; 4];
            match reader.read_exact(&mut size_buf) {
                Ok(()) => {},
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(BlockDBError::IoError(e)),
            }
            
            let size = u32::from_be_bytes(size_buf) as usize;
            
            // Read entry
            let mut entry_buf = vec![0u8; size];
            reader.read_exact(&mut entry_buf)
                .map_err(|e| BlockDBError::IoError(e))?;
            
            let entry: TransactionLogEntry = bincode::deserialize(&entry_buf)
                .map_err(|e| BlockDBError::SerializationError(e))?;
            
            entries.push(entry);
        }
        
        Ok(entries)
    }
    
    pub fn truncate(&mut self) -> Result<(), BlockDBError> {
        self.file.get_mut().set_len(0)
            .map_err(|e| BlockDBError::IoError(e))?;
        
        self.file.get_mut().seek(SeekFrom::Start(0))
            .map_err(|e| BlockDBError::IoError(e))?;
        
        Ok(())
    }
}