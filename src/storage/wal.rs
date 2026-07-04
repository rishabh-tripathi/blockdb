use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Write, Seek, SeekFrom};
use std::path::Path;
use crate::storage::Record;

/// Per-record framing: [payload_len: u32 BE][crc32: u32 BE][payload: bincode(Record)].
/// The checksum lets recovery detect torn/partial writes (e.g. after a crash mid-append)
/// and stop cleanly at the last intact record instead of aborting or returning garbage.
const WAL_RECORD_HEADER_SIZE: u64 = 8;

/// Durability mode for the write-ahead log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    /// Call `fsync` after every append. Acknowledged writes survive power loss.
    Always,
    /// Only flush to the OS page cache on append; rely on periodic `sync()` calls.
    /// Higher throughput, but the last writes can be lost on power loss.
    Periodic,
}

#[derive(Debug)]
pub struct WriteAheadLog {
    file: File,
    path: String,
    offset: u64,
    sync_mode: SyncMode,
}

impl WriteAheadLog {
    pub fn new(data_dir: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_sync_mode(data_dir, SyncMode::Always)
    }

    pub fn with_sync_mode(data_dir: &str, sync_mode: SyncMode) -> Result<Self, Box<dyn std::error::Error>> {
        let path = format!("{}/wal.log", data_dir);

        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .append(true)
            .open(&path)?;

        let offset = file.metadata()?.len();

        Ok(WriteAheadLog {
            file,
            path,
            offset,
            sync_mode,
        })
    }

    pub fn append(&mut self, record: &Record) -> Result<(), Box<dyn std::error::Error>> {
        let serialized = bincode::serialize(record)?;
        let record_size = serialized.len() as u32;
        let crc = crc32(&serialized);

        // Write the framed record. The append-mode file guarantees the write lands
        // at the end; the length + CRC header makes partial writes detectable on recovery.
        self.file.write_all(&record_size.to_be_bytes())?;
        self.file.write_all(&crc.to_be_bytes())?;
        self.file.write_all(&serialized)?;
        self.file.flush()?;

        if self.sync_mode == SyncMode::Always {
            self.file.sync_data()?;
        }

        self.offset += WAL_RECORD_HEADER_SIZE + serialized.len() as u64;

        Ok(())
    }

    /// Replay the log, returning every intact record. A torn or corrupt tail record
    /// (partial write from a crash) is treated as end-of-log rather than an error, so
    /// recovery never fails because of an interrupted final append.
    pub fn recover(&self) -> Result<Vec<Record>, Box<dyn std::error::Error>> {
        let file = File::open(&self.path)?;
        let file_len = file.metadata()?.len();
        let mut reader = BufReader::new(file);
        let mut records = Vec::new();
        let mut position: u64 = 0;

        loop {
            if position + WAL_RECORD_HEADER_SIZE > file_len {
                // Not enough bytes left for another header: clean end (or torn tail).
                break;
            }

            let mut header = [0u8; WAL_RECORD_HEADER_SIZE as usize];
            if reader.read_exact(&mut header).is_err() {
                break;
            }
            let record_size = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
            let expected_crc = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);

            if position + WAL_RECORD_HEADER_SIZE + record_size as u64 > file_len {
                // Payload was not fully written before the crash: stop here.
                break;
            }

            let mut record_buf = vec![0u8; record_size];
            if reader.read_exact(&mut record_buf).is_err() {
                break;
            }

            if crc32(&record_buf) != expected_crc {
                // Corruption in the tail record; treat everything after as lost.
                break;
            }

            match bincode::deserialize::<Record>(&record_buf) {
                Ok(record) => records.push(record),
                Err(_) => break,
            }

            position += WAL_RECORD_HEADER_SIZE + record_size as u64;
        }

        Ok(records)
    }

    /// Truncate the log to empty. Called after a memtable flush makes the logged
    /// records durable in an SSTable, so the WAL does not grow without bound.
    pub fn truncate(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.sync_all()?;
        self.offset = 0;
        Ok(())
    }

    pub fn sync(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.file.flush()?;
        self.file.sync_all()?;
        Ok(())
    }

    /// Clear all WAL data and reset to empty state.
    pub fn clear(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.truncate()
    }

    pub fn is_empty(&self) -> bool {
        self.offset == 0
    }
}

/// Standard CRC-32 (IEEE 802.3 polynomial), computed without pulling in an external crate.
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_record(seq: u64) -> Record {
        Record {
            key: format!("key{}", seq).into_bytes(),
            value: format!("val{}", seq).into_bytes(),
            timestamp: seq,
            sequence_number: seq,
            hash: vec![seq as u8; 4],
        }
    }

    #[test]
    fn test_append_and_recover() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_string_lossy().to_string();

        let mut wal = WriteAheadLog::new(&path).unwrap();
        wal.append(&sample_record(1)).unwrap();
        wal.append(&sample_record(2)).unwrap();

        let recovered = wal.recover().unwrap();
        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered[0].sequence_number, 1);
        assert_eq!(recovered[1].sequence_number, 2);
    }

    #[test]
    fn test_torn_tail_is_ignored() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_string_lossy().to_string();

        {
            let mut wal = WriteAheadLog::new(&path).unwrap();
            wal.append(&sample_record(1)).unwrap();
        }

        // Simulate a torn write by appending a truncated frame directly.
        let wal_path = format!("{}/wal.log", path);
        let mut f = OpenOptions::new().append(true).open(&wal_path).unwrap();
        f.write_all(&100u32.to_be_bytes()).unwrap(); // claims 100 bytes
        f.write_all(&0u32.to_be_bytes()).unwrap(); // crc
        f.write_all(b"short").unwrap(); // but only 5 bytes follow
        f.sync_all().unwrap();

        let wal = WriteAheadLog::new(&path).unwrap();
        let recovered = wal.recover().unwrap();
        assert_eq!(recovered.len(), 1, "torn tail frame must be skipped");
        assert_eq!(recovered[0].sequence_number, 1);
    }

    #[test]
    fn test_truncate_empties_log() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_string_lossy().to_string();

        let mut wal = WriteAheadLog::new(&path).unwrap();
        wal.append(&sample_record(1)).unwrap();
        wal.truncate().unwrap();

        assert!(wal.is_empty());
        assert_eq!(wal.recover().unwrap().len(), 0);
    }
}
