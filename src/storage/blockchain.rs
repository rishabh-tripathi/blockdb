use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use sha2::{Sha256, Digest};
use serde::{Serialize, Deserialize};
use crate::storage::Record;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub index: u64,
    pub timestamp: u64,
    pub previous_hash: Vec<u8>,
    pub merkle_root: Vec<u8>,
    pub records: Vec<Record>,
    pub hash: Vec<u8>,
    pub nonce: u64,
}

impl Block {
    pub fn new(index: u64, previous_hash: Vec<u8>, records: Vec<Record>) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let merkle_root = Self::calculate_merkle_root(&records);
        let mut block = Block {
            index,
            timestamp,
            previous_hash,
            merkle_root,
            records,
            hash: Vec::new(),
            nonce: 0,
        };
        
        block.hash = block.calculate_hash();
        block
    }

    fn calculate_hash(&self) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(&self.index.to_be_bytes());
        hasher.update(&self.timestamp.to_be_bytes());
        hasher.update(&self.previous_hash);
        hasher.update(&self.merkle_root);
        hasher.update(&self.nonce.to_be_bytes());
        
        for record in &self.records {
            hasher.update(&record.hash);
        }
        
        hasher.finalize().to_vec()
    }

    fn calculate_merkle_root(records: &[Record]) -> Vec<u8> {
        if records.is_empty() {
            return vec![0u8; 32];
        }
        
        let mut hashes: Vec<Vec<u8>> = records.iter().map(|r| r.hash.clone()).collect();
        
        while hashes.len() > 1 {
            let mut next_level = Vec::new();
            
            for chunk in hashes.chunks(2) {
                let mut hasher = Sha256::new();
                hasher.update(&chunk[0]);
                if chunk.len() > 1 {
                    hasher.update(&chunk[1]);
                } else {
                    hasher.update(&chunk[0]);
                }
                next_level.push(hasher.finalize().to_vec());
            }
            
            hashes = next_level;
        }
        
        hashes.into_iter().next().unwrap_or_else(|| vec![0u8; 32])
    }

    pub fn verify_integrity(&self) -> bool {
        let calculated_hash = self.calculate_hash();
        let calculated_merkle = Self::calculate_merkle_root(&self.records);
        
        self.hash == calculated_hash && self.merkle_root == calculated_merkle
    }
}

#[derive(Debug)]
pub struct BlockChain {
    blocks: Vec<Block>,
    pending_records: VecDeque<Record>,
    batch_size: usize,
    file_path: String,
}

impl BlockChain {
    pub fn new(data_dir: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let file_path = format!("{}/blockchain.dat", data_dir);
        let mut blockchain = BlockChain {
            blocks: Vec::new(),
            pending_records: VecDeque::new(),
            batch_size: 1000,
            file_path,
        };
        
        blockchain.load_from_disk()?;
        
        if blockchain.blocks.is_empty() {
            let genesis_block = Block::new(0, vec![0u8; 32], Vec::new());
            blockchain.blocks.push(genesis_block);
            blockchain.save_to_disk()?;
        }
        
        Ok(blockchain)
    }

    pub fn add_record(&mut self, record: Record) -> Result<(), Box<dyn std::error::Error>> {
        self.pending_records.push_back(record);
        
        if self.pending_records.len() >= self.batch_size {
            self.create_block()?;
        }
        
        Ok(())
    }

    fn create_block(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.pending_records.is_empty() {
            return Ok(());
        }
        
        let records: Vec<Record> = self.pending_records.drain(..).collect();
        let previous_hash = self.blocks.last().unwrap().hash.clone();
        let index = self.blocks.len() as u64;
        
        let block = Block::new(index, previous_hash, records);
        // Append-only: write just this new block's frame to disk instead of
        // rewriting the entire chain (which made block creation O(n) and the whole
        // chain's lifetime O(n^2)).
        self.append_block_to_disk(&block)?;
        self.blocks.push(block);

        Ok(())
    }

    pub fn verify_chain(&self) -> Result<bool, Box<dyn std::error::Error>> {
        if self.blocks.is_empty() {
            return Ok(true);
        }
        
        for i in 1..self.blocks.len() {
            let current_block = &self.blocks[i];
            let previous_block = &self.blocks[i - 1];
            
            if !current_block.verify_integrity() {
                return Ok(false);
            }
            
            if current_block.previous_hash != previous_block.hash {
                return Ok(false);
            }
            
            if current_block.index != previous_block.index + 1 {
                return Ok(false);
            }
        }
        
        Ok(true)
    }

    pub fn get_block(&self, index: u64) -> Option<&Block> {
        self.blocks.get(index as usize)
    }

    pub fn get_latest_block(&self) -> Option<&Block> {
        self.blocks.last()
    }

    pub fn get_chain_length(&self) -> usize {
        self.blocks.len()
    }

    pub fn force_create_block(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.create_block()
    }

    /// On-disk format: a sequence of length-prefixed frames, one per block:
    /// `[len: u32 BE][bincode(Block)]`. Appending a block is O(block) and loading
    /// is O(chain). A torn tail frame (crash mid-append) is dropped on load.
    ///
    /// Full rewrite of the whole chain — only used to initialize/reset the file
    /// (genesis, `clear`); steady-state growth goes through `append_block_to_disk`.
    fn save_to_disk(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut file = BufWriter::new(
            OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&self.file_path)?,
        );
        for block in &self.blocks {
            Self::write_frame(&mut file, block)?;
        }
        file.flush()?;
        file.get_ref().sync_all()?;
        Ok(())
    }

    /// Append a single block frame to the end of the file (append-only, no rewrite).
    fn append_block_to_disk(&self, block: &Block) -> Result<(), Box<dyn std::error::Error>> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)?;
        Self::write_frame(&mut file, block)?;
        file.flush()?;
        file.sync_data()?;
        Ok(())
    }

    fn write_frame<W: Write>(w: &mut W, block: &Block) -> Result<(), Box<dyn std::error::Error>> {
        let payload = bincode::serialize(block)?;
        w.write_all(&(payload.len() as u32).to_be_bytes())?;
        w.write_all(&payload)?;
        Ok(())
    }

    fn load_from_disk(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if !Path::new(&self.file_path).exists() {
            return Ok(());
        }

        let file = File::open(&self.file_path)?;
        let file_len = file.metadata()?.len();
        let mut reader = BufReader::new(file);
        let mut blocks = Vec::new();
        let mut pos: u64 = 0;

        loop {
            if pos + 4 > file_len {
                break;
            }
            let mut len_buf = [0u8; 4];
            if reader.read_exact(&mut len_buf).is_err() {
                break;
            }
            let len = u32::from_be_bytes(len_buf) as u64;
            if pos + 4 + len > file_len {
                break; // torn tail frame
            }
            let mut payload = vec![0u8; len as usize];
            if reader.read_exact(&mut payload).is_err() {
                break;
            }
            match bincode::deserialize::<Block>(&payload) {
                Ok(block) => blocks.push(block),
                Err(_) => break, // corrupt tail
            }
            pos += 4 + len;
        }

        if !blocks.is_empty() {
            self.blocks = blocks;
        }
        Ok(())
    }

    pub fn get_record_proof(&self, record_hash: &[u8]) -> Option<Vec<Vec<u8>>> {
        for block in &self.blocks {
            if let Some(index) = block.records.iter().position(|r| r.hash == record_hash) {
                return Some(self.generate_merkle_proof(&block.records, index));
            }
        }
        None
    }

    fn generate_merkle_proof(&self, records: &[Record], target_index: usize) -> Vec<Vec<u8>> {
        let mut proof = Vec::new();
        let mut hashes: Vec<Vec<u8>> = records.iter().map(|r| r.hash.clone()).collect();
        let mut index = target_index;
        
        while hashes.len() > 1 {
            if index % 2 == 0 && index + 1 < hashes.len() {
                proof.push(hashes[index + 1].clone());
            } else if index % 2 == 1 {
                proof.push(hashes[index - 1].clone());
            }
            
            let mut next_level = Vec::new();
            for chunk in hashes.chunks(2) {
                let mut hasher = Sha256::new();
                hasher.update(&chunk[0]);
                if chunk.len() > 1 {
                    hasher.update(&chunk[1]);
                } else {
                    hasher.update(&chunk[0]);
                }
                next_level.push(hasher.finalize().to_vec());
            }
            
            hashes = next_level;
            index /= 2;
        }
        
        proof
    }

    /// Clear all blockchain data and reset to genesis block
    pub fn clear(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Clear all blocks and pending records
        self.blocks.clear();
        self.pending_records.clear();
        
        // Create new genesis block
        let genesis_block = Block::new(0, vec![0u8; 32], Vec::new());
        self.blocks.push(genesis_block);
        
        // Save to disk
        self.save_to_disk()?;
        
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Record;
    use tempfile::TempDir;

    fn rec(seq: u64) -> Record {
        Record {
            key: format!("key{}", seq).into_bytes(),
            value: format!("val{}", seq).into_bytes(),
            timestamp: seq,
            sequence_number: seq,
            hash: vec![seq as u8; 32],
        }
    }

    #[test]
    fn many_blocks_append_and_reload() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_string_lossy().to_string();

        {
            let mut chain = BlockChain::new(&path).unwrap();
            for i in 0..50 {
                chain.add_record(rec(i)).unwrap();
                chain.force_create_block().unwrap();
            }
            // genesis + 50 blocks
            assert_eq!(chain.get_chain_length(), 51);
            assert!(chain.verify_chain().unwrap());
        }

        // Reload from the append-only file: every block must come back intact.
        let chain = BlockChain::new(&path).unwrap();
        assert_eq!(chain.get_chain_length(), 51);
        assert!(chain.verify_chain().unwrap());
        assert_eq!(chain.get_block(50).unwrap().index, 50);
    }

    #[test]
    fn torn_tail_frame_is_dropped_on_load() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_string_lossy().to_string();
        let file_path = {
            let mut chain = BlockChain::new(&path).unwrap();
            chain.add_record(rec(1)).unwrap();
            chain.force_create_block().unwrap();
            chain.file_path.clone()
        };

        // Append a torn frame: claims 9999 bytes, writes only a few.
        let mut f = OpenOptions::new().append(true).open(&file_path).unwrap();
        f.write_all(&9999u32.to_be_bytes()).unwrap();
        f.write_all(b"partial").unwrap();
        f.sync_all().unwrap();

        // The intact blocks (genesis + 1) survive; the torn frame is ignored.
        let chain = BlockChain::new(&path).unwrap();
        assert_eq!(chain.get_chain_length(), 2);
        assert!(chain.verify_chain().unwrap());
    }
}
