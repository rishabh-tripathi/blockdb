use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write, Seek, SeekFrom};
use std::path::Path;
use serde::{Serialize, Deserialize};
use crate::storage::Record;

const WAL_HEADER_SIZE: usize = 8;
const WAL_RECORD_HEADER_SIZE: usize = 4;

#[derive(Debug)]
pub struct WriteAheadLog {
    file: BufWriter<File>,
    path: String,
    offset: u64,
}

impl WriteAheadLog {
    pub fn new(data_dir: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let path = format!("{}/wal.log", data_dir);
        
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(&path)?;
        
        let mut wal = WriteAheadLog {
            file: BufWriter::new(file),
            path,
            offset: 0,
        };
        
        wal.offset = wal.file.get_ref().metadata()?.len();
        
        Ok(wal)
    }

    pub fn append(&mut self, record: &Record) -> Result<(), Box<dyn std::error::Error>> {
        let serialized = bincode::serialize(record)?;
        let record_size = serialized.len() as u32;
        
        self.file.write_all(&record_size.to_be_bytes())?;
        self.file.write_all(&serialized)?;
        self.file.flush()?;
        
        self.offset += WAL_RECORD_HEADER_SIZE as u64 + serialized.len() as u64;
        
        Ok(())
    }

    pub fn recover(&self) -> Result<Vec<Record>, Box<dyn std::error::Error>> {
        let file = File::open(&self.path)?;
        let mut reader = BufReader::new(file);
        let mut records = Vec::new();
        
        loop {
            let mut size_buf = [0u8; 4];
            match reader.read_exact(&mut size_buf) {
                Ok(()) => {},
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(Box::new(e)),
            }
            
            let record_size = u32::from_be_bytes(size_buf) as usize;
            let mut record_buf = vec![0u8; record_size];
            reader.read_exact(&mut record_buf)?;
            
            let record: Record = bincode::deserialize(&record_buf)?;
            records.push(record);
        }
        
        Ok(records)
    }

    pub fn truncate(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.file.get_mut().set_len(0)?;
        self.file.get_mut().seek(SeekFrom::Start(0))?;
        self.offset = 0;
        Ok(())
    }

    pub fn sync(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.file.flush()?;
        self.file.get_mut().sync_all()?;
        Ok(())
    }

    /// Clear all WAL data and reset to empty state
    pub fn clear(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Truncate the file to zero length
        self.file.get_mut().set_len(0)?;
        self.file.get_mut().seek(SeekFrom::Start(0))?;
        self.file.flush()?;
        self.offset = 0;
        Ok(())
    }
}