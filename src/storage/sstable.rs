use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write, Seek, SeekFrom};
use std::path::Path;
use serde::{Serialize, Deserialize};
use crate::storage::{Record, memtable::MemTable};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub key: Vec<u8>,
    pub offset: u64,
    pub size: u32,
}

#[derive(Debug)]
pub struct SSTable {
    path: String,
    index: BTreeMap<Vec<u8>, IndexEntry>,
    file: File,
}

impl SSTable {
    pub fn create_from_memtable(path: &str, memtable: &MemTable) -> Result<Self, Box<dyn std::error::Error>> {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .read(true)
            .open(path)?;

        let mut index = BTreeMap::new();
        let mut offset = 0u64;

        for (key, record) in memtable.iter() {
            let serialized = bincode::serialize(record)?;
            let size = serialized.len() as u32;
            
            file.write_all(&size.to_be_bytes())?;
            file.write_all(&serialized)?;
            
            index.insert(key.clone(), IndexEntry {
                key: key.clone(),
                offset,
                size,
            });
            
            offset += 4 + serialized.len() as u64;
        }

        let index_offset = offset;
        let index_data = bincode::serialize(&index)?;
        file.write_all(&index_data)?;
        file.write_all(&index_offset.to_be_bytes())?;
        file.write_all(&(index_data.len() as u64).to_be_bytes())?;
        
        file.flush()?;
        file.seek(SeekFrom::Start(0))?;

        Ok(SSTable {
            path: path.to_string(),
            index,
            file,
        })
    }

    pub fn open(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut file = File::open(path)?;
        let _file_size = file.metadata()?.len();
        
        file.seek(SeekFrom::End(-16))?;
        let mut footer = [0u8; 16];
        file.read_exact(&mut footer)?;
        
        let index_offset = u64::from_be_bytes([footer[0], footer[1], footer[2], footer[3], footer[4], footer[5], footer[6], footer[7]]);
        let index_size = u64::from_be_bytes([footer[8], footer[9], footer[10], footer[11], footer[12], footer[13], footer[14], footer[15]]);
        
        file.seek(SeekFrom::Start(index_offset))?;
        let mut index_data = vec![0u8; index_size as usize];
        file.read_exact(&mut index_data)?;
        
        let index: BTreeMap<Vec<u8>, IndexEntry> = bincode::deserialize(&index_data)?;
        
        file.seek(SeekFrom::Start(0))?;
        
        Ok(SSTable {
            path: path.to_string(),
            index,
            file,
        })
    }

    pub fn get(&mut self, key: &[u8]) -> Result<Option<Record>, Box<dyn std::error::Error>> {
        if let Some(entry) = self.index.get(key) {
            self.file.seek(SeekFrom::Start(entry.offset))?;
            
            let mut size_buf = [0u8; 4];
            self.file.read_exact(&mut size_buf)?;
            let size = u32::from_be_bytes(size_buf);
            
            let mut record_buf = vec![0u8; size as usize];
            self.file.read_exact(&mut record_buf)?;
            
            let record: Record = bincode::deserialize(&record_buf)?;
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    pub fn scan_range(&mut self, start: &[u8], end: &[u8]) -> Result<Vec<Record>, Box<dyn std::error::Error>> {
        let mut results = Vec::new();
        
        for (_key, entry) in self.index.range(start.to_vec()..end.to_vec()) {
            self.file.seek(SeekFrom::Start(entry.offset))?;
            
            let mut size_buf = [0u8; 4];
            self.file.read_exact(&mut size_buf)?;
            let size = u32::from_be_bytes(size_buf);
            
            let mut record_buf = vec![0u8; size as usize];
            self.file.read_exact(&mut record_buf)?;
            
            let record: Record = bincode::deserialize(&record_buf)?;
            results.push(record);
        }
        
        Ok(results)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Vec<u8>> {
        self.index.keys()
    }

    pub fn size(&self) -> usize {
        self.index.len()
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn contains_key(&self, key: &[u8]) -> bool {
        self.index.contains_key(key)
    }

    pub fn first_key(&self) -> Option<&Vec<u8>> {
        self.index.keys().next()
    }

    pub fn last_key(&self) -> Option<&Vec<u8>> {
        self.index.keys().last()
    }
}