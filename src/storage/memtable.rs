use std::collections::BTreeMap;
use crate::storage::Record;

#[derive(Debug)]
pub struct MemTable {
    data: BTreeMap<Vec<u8>, Record>,
    size: usize,
}

impl MemTable {
    pub fn new() -> Self {
        MemTable {
            data: BTreeMap::new(),
            size: 0,
        }
    }

    pub fn insert(&mut self, record: Record) {
        let key = record.key.clone();
        let record_size = self.calculate_record_size(&record);
        
        if let Some(old_record) = self.data.insert(key, record) {
            self.size -= self.calculate_record_size(&old_record);
        }
        
        self.size += record_size;
    }

    pub fn get(&self, key: &[u8]) -> Option<&Record> {
        self.data.get(key)
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Vec<u8>, &Record)> {
        self.data.iter()
    }

    pub fn keys(&self) -> impl Iterator<Item = &Vec<u8>> {
        self.data.keys()
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    fn calculate_record_size(&self, record: &Record) -> usize {
        record.key.len() + 
        record.value.len() + 
        record.hash.len() + 
        8 + // timestamp
        8 + // sequence_number
        32  // overhead estimation
    }

    pub fn clear(&mut self) {
        self.data.clear();
        self.size = 0;
    }

    pub fn range(&self, start: &[u8], end: &[u8]) -> impl Iterator<Item = (&Vec<u8>, &Record)> {
        self.data.range(start.to_vec()..end.to_vec())
    }

    pub fn get_latest_by_prefix(&self, prefix: &[u8]) -> Option<&Record> {
        self.data
            .range(prefix.to_vec()..)
            .take_while(|(key, _)| key.starts_with(prefix))
            .last()
            .map(|(_, record)| record)
    }
}