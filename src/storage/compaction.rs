use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use crate::storage::{Record, sstable::SSTable};

pub struct Compactor {
    data_dir: String,
    levels: Vec<Vec<String>>,
    max_level_size: Vec<usize>,
}

impl Compactor {
    pub fn new(data_dir: &str) -> Self {
        Compactor {
            data_dir: data_dir.to_string(),
            levels: vec![Vec::new(); 7],
            max_level_size: vec![10, 100, 1000, 10000, 100000, 1000000, 10000000],
        }
    }

    pub fn add_sstable(&mut self, sstable_path: String, level: usize) {
        if level < self.levels.len() {
            self.levels[level].push(sstable_path);
        }
    }

    pub fn needs_compaction(&self, level: usize) -> bool {
        if level >= self.levels.len() {
            return false;
        }
        
        self.levels[level].len() > self.max_level_size[level]
    }

    pub fn compact_level(&mut self, level: usize) -> Result<(), Box<dyn std::error::Error>> {
        if level >= self.levels.len() - 1 {
            return Ok(());
        }

        let mut records = BTreeMap::new();
        let mut files_to_remove = Vec::new();

        for sstable_path in &self.levels[level] {
            let mut sstable = SSTable::open(sstable_path)?;
            
            let keys: Vec<_> = sstable.iter().cloned().collect();
            for key in keys {
                if let Some(record) = sstable.get(&key)? {
                    records.insert(key, record);
                }
            }
            
            files_to_remove.push(sstable_path.clone());
        }

        if !records.is_empty() {
            let new_sstable_path = format!("{}/compacted_{}_{}.sst", 
                self.data_dir, 
                level + 1,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );

            self.create_sstable_from_records(&new_sstable_path, records)?;
            self.levels[level + 1].push(new_sstable_path);
        }

        for file_path in files_to_remove {
            fs::remove_file(file_path)?;
        }
        
        self.levels[level].clear();

        if self.needs_compaction(level + 1) {
            self.compact_level(level + 1)?;
        }

        Ok(())
    }

    fn create_sstable_from_records(
        &self,
        path: &str,
        records: BTreeMap<Vec<u8>, Record>
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::fs::OpenOptions;
        use std::io::Write;
        
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;

        let mut index = BTreeMap::new();
        let mut offset = 0u64;

        for (key, record) in records {
            let serialized = bincode::serialize(&record)?;
            let size = serialized.len() as u32;
            
            file.write_all(&size.to_be_bytes())?;
            file.write_all(&serialized)?;
            
            index.insert(key.clone(), crate::storage::sstable::IndexEntry {
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
        
        Ok(())
    }

    pub fn get_level_info(&self) -> Vec<usize> {
        self.levels.iter().map(|level| level.len()).collect()
    }

    pub fn cleanup_empty_levels(&mut self) {
        for level in &mut self.levels {
            level.retain(|path| Path::new(path).exists());
        }
    }
}