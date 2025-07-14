use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::error::BlockDBError;
use super::{BlockDB, BlockDBConfig, Record};

pub type CollectionId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionMetadata {
    pub id: CollectionId,
    pub name: String,
    pub description: Option<String>,
    pub created_at: u64,
    pub created_by: Option<String>,
    pub schema: Option<CollectionSchema>,
    pub settings: CollectionSettings,
    pub stats: CollectionStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSchema {
    pub version: u32,
    pub fields: HashMap<String, FieldDefinition>,
    pub required_fields: Vec<String>,
    pub indexes: Vec<IndexDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDefinition {
    pub field_type: FieldType,
    pub required: bool,
    pub default_value: Option<String>,
    pub validation_rules: Vec<ValidationRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FieldType {
    String,
    Integer,
    Float,
    Boolean,
    Array,
    Object,
    Binary,
    Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationRule {
    MinLength(usize),
    MaxLength(usize),
    Pattern(String),
    MinValue(f64),
    MaxValue(f64),
    OneOf(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexDefinition {
    pub name: String,
    pub fields: Vec<String>,
    pub unique: bool,
    pub sparse: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSettings {
    pub max_document_size: Option<usize>,
    pub ttl_seconds: Option<u64>,
    pub compression_enabled: bool,
    pub encryption_enabled: bool,
    pub replication_factor: u32,
    pub read_concern: ReadConcern,
    pub write_concern: WriteConcern,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReadConcern {
    Local,
    Majority,
    Linearizable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WriteConcern {
    Unacknowledged,
    Acknowledged,
    Majority,
    Custom(u32), // Number of nodes that must acknowledge
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionStats {
    pub document_count: u64,
    pub total_size_bytes: u64,
    pub index_size_bytes: u64,
    pub last_updated: u64,
    pub operations_count: u64,
}

impl Default for CollectionSettings {
    fn default() -> Self {
        Self {
            max_document_size: Some(16 * 1024 * 1024), // 16MB
            ttl_seconds: None,
            compression_enabled: false,
            encryption_enabled: false,
            replication_factor: 3,
            read_concern: ReadConcern::Local,
            write_concern: WriteConcern::Acknowledged,
        }
    }
}

impl Default for CollectionStats {
    fn default() -> Self {
        Self {
            document_count: 0,
            total_size_bytes: 0,
            index_size_bytes: 0,
            last_updated: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            operations_count: 0,
        }
    }
}

impl CollectionMetadata {
    pub fn new(name: String, created_by: Option<String>) -> Self {
        let id = format!("col_{}", Uuid::new_v4());
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            id,
            name,
            description: None,
            created_at,
            created_by,
            schema: None,
            settings: CollectionSettings::default(),
            stats: CollectionStats::default(),
        }
    }

    pub fn with_schema(mut self, schema: CollectionSchema) -> Self {
        self.schema = Some(schema);
        self
    }

    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    pub fn with_settings(mut self, settings: CollectionSettings) -> Self {
        self.settings = settings;
        self
    }
}

#[derive(Debug)]
pub struct Collection {
    pub metadata: Arc<RwLock<CollectionMetadata>>,
    pub storage: Arc<RwLock<BlockDB>>,
    pub indexes: Arc<RwLock<HashMap<String, Vec<Vec<u8>>>>>, // index_name -> list of keys
}

impl Collection {
    pub fn new(metadata: CollectionMetadata, config: BlockDBConfig) -> Result<Self, BlockDBError> {
        // Create storage with collection-specific data directory
        let mut collection_config = config.clone();
        collection_config.data_dir = format!("{}/collections/{}", config.data_dir, metadata.id);

        let storage = BlockDB::new(collection_config)?;

        Ok(Self {
            metadata: Arc::new(RwLock::new(metadata)),
            storage: Arc::new(RwLock::new(storage)),
            indexes: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<(), BlockDBError> {
        // Validate against schema if present
        self.validate_document(value)?;

        // Store in underlying BlockDB
        let mut storage = self.storage.write().unwrap();
        storage.put(key, value)?;

        // Update statistics
        self.update_stats(key, value, true)?;

        // Update indexes
        self.update_indexes(key, value)?;

        Ok(())
    }

    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, BlockDBError> {
        let storage = self.storage.read().unwrap();
        storage.get(key).map_err(BlockDBError::from)
    }

    pub fn delete(&self, key: &[u8]) -> Result<(), BlockDBError> {
        // Note: In append-only architecture, this marks as deleted
        // For now, return error as delete is not supported in append-only
        Err(BlockDBError::ApiError(
            "Delete operations not supported in append-only database".to_string(),
        ))
    }

    pub fn list_keys(&self, prefix: Option<&[u8]>, limit: Option<usize>) -> Result<Vec<Vec<u8>>, BlockDBError> {
        // Implementation would scan through keys with optional prefix filter
        let storage = self.storage.read().unwrap();
        
        // For now, return empty list as this requires memtable/sstable scanning
        // In full implementation, this would scan through all keys
        Ok(Vec::new())
    }

    pub fn count_documents(&self) -> Result<u64, BlockDBError> {
        let metadata = self.metadata.read().unwrap();
        Ok(metadata.stats.document_count)
    }

    pub fn get_stats(&self) -> Result<CollectionStats, BlockDBError> {
        let metadata = self.metadata.read().unwrap();
        Ok(metadata.stats.clone())
    }

    pub fn create_index(&self, index_def: IndexDefinition) -> Result<(), BlockDBError> {
        // Validate index definition
        if index_def.fields.is_empty() {
            return Err(BlockDBError::ApiError("Index must have at least one field".to_string()));
        }

        // Add index to metadata
        {
            let mut metadata = self.metadata.write().unwrap();
            if let Some(ref mut schema) = metadata.schema {
                schema.indexes.push(index_def.clone());
            } else {
                // Create schema if it doesn't exist
                let mut schema = CollectionSchema {
                    version: 1,
                    fields: HashMap::new(),
                    required_fields: Vec::new(),
                    indexes: vec![index_def.clone()],
                };
                metadata.schema = Some(schema);
            }
        }

        // Initialize index storage
        {
            let mut indexes = self.indexes.write().unwrap();
            indexes.insert(index_def.name.clone(), Vec::new());
        }

        println!("✅ Index '{}' created for collection", index_def.name);
        Ok(())
    }

    pub fn drop_index(&self, index_name: &str) -> Result<(), BlockDBError> {
        // Remove from metadata
        {
            let mut metadata = self.metadata.write().unwrap();
            if let Some(ref mut schema) = metadata.schema {
                schema.indexes.retain(|idx| idx.name != index_name);
            }
        }

        // Remove index storage
        {
            let mut indexes = self.indexes.write().unwrap();
            indexes.remove(index_name);
        }

        println!("✅ Index '{}' dropped from collection", index_name);
        Ok(())
    }

    fn validate_document(&self, _value: &[u8]) -> Result<(), BlockDBError> {
        // In a full implementation, this would validate the document against the schema
        // For now, just return Ok
        Ok(())
    }

    fn update_stats(&self, key: &[u8], value: &[u8], is_new: bool) -> Result<(), BlockDBError> {
        let mut metadata = self.metadata.write().unwrap();
        
        if is_new {
            metadata.stats.document_count += 1;
        }
        
        metadata.stats.total_size_bytes += key.len() as u64 + value.len() as u64;
        metadata.stats.operations_count += 1;
        metadata.stats.last_updated = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Ok(())
    }

    fn update_indexes(&self, key: &[u8], _value: &[u8]) -> Result<(), BlockDBError> {
        // In a full implementation, this would extract index fields from the document
        // and update the appropriate indexes
        
        let mut indexes = self.indexes.write().unwrap();
        
        // For each index, add the key to the index
        for (index_name, index_keys) in indexes.iter_mut() {
            index_keys.push(key.to_vec());
        }

        Ok(())
    }

    pub fn verify_integrity(&self) -> Result<bool, BlockDBError> {
        let storage = self.storage.read().unwrap();
        storage.verify_integrity().map_err(BlockDBError::from)
    }

    /// Flush all data in this collection
    pub fn flush(&self) -> Result<(), BlockDBError> {
        let mut storage = self.storage.write().unwrap();
        storage.flush_all().map_err(BlockDBError::from)?;
        
        // Reset collection statistics
        {
            let mut metadata = self.metadata.write().unwrap();
            metadata.stats = CollectionStats::default();
        }
        
        // Clear indexes
        {
            let mut indexes = self.indexes.write().unwrap();
            indexes.clear();
        }
        
        Ok(())
    }
}

/// CollectionManager coordinates multiple collections within a single node
#[derive(Debug)]
pub struct CollectionManager {
    collections: Arc<RwLock<HashMap<CollectionId, Collection>>>,
    metadata_store: Arc<RwLock<HashMap<CollectionId, CollectionMetadata>>>,
    config: BlockDBConfig,
    default_collection: Option<CollectionId>,
}

impl CollectionManager {
    pub fn new(config: BlockDBConfig) -> Result<Self, BlockDBError> {
        // Ensure collections directory exists
        std::fs::create_dir_all(format!("{}/collections", config.data_dir))
            .map_err(|e| BlockDBError::IoError(e))?;

        let manager = Self {
            collections: Arc::new(RwLock::new(HashMap::new())),
            metadata_store: Arc::new(RwLock::new(HashMap::new())),
            config,
            default_collection: None,
        };

        // Load existing collections on startup
        manager.load_existing_collections()?;

        Ok(manager)
    }

    pub fn create_collection(
        &self,
        name: String,
        schema: Option<CollectionSchema>,
        settings: Option<CollectionSettings>,
        created_by: Option<String>,
    ) -> Result<CollectionId, BlockDBError> {
        // Check if collection with this name already exists
        {
            let metadata_store = self.metadata_store.read().unwrap();
            for metadata in metadata_store.values() {
                if metadata.name == name {
                    return Err(BlockDBError::ApiError(format!(
                        "Collection with name '{}' already exists",
                        name
                    )));
                }
            }
        }

        // Create metadata
        let mut metadata = CollectionMetadata::new(name.clone(), created_by);
        
        if let Some(schema) = schema {
            metadata = metadata.with_schema(schema);
        }
        
        if let Some(settings) = settings {
            metadata = metadata.with_settings(settings);
        }

        let collection_id = metadata.id.clone();

        // Create collection
        let collection = Collection::new(metadata.clone(), self.config.clone())?;

        // Store in manager
        {
            let mut collections = self.collections.write().unwrap();
            collections.insert(collection_id.clone(), collection);
        }

        {
            let mut metadata_store = self.metadata_store.write().unwrap();
            metadata_store.insert(collection_id.clone(), metadata);
        }

        // Persist metadata to disk
        self.persist_collection_metadata(&collection_id)?;

        println!("✅ Collection '{}' created with ID: {}", name, collection_id);
        Ok(collection_id)
    }

    pub fn drop_collection(&self, collection_id: &str) -> Result<(), BlockDBError> {
        // Remove from memory
        let collection_existed = {
            let mut collections = self.collections.write().unwrap();
            collections.remove(collection_id).is_some()
        };

        let metadata_existed = {
            let mut metadata_store = self.metadata_store.write().unwrap();
            metadata_store.remove(collection_id).is_some()
        };

        if !collection_existed || !metadata_existed {
            return Err(BlockDBError::ApiError(format!(
                "Collection '{}' does not exist",
                collection_id
            )));
        }

        // Remove metadata file
        let metadata_path = format!("{}/collections/{}/metadata.toml", self.config.data_dir, collection_id);
        if std::path::Path::new(&metadata_path).exists() {
            std::fs::remove_file(&metadata_path)
                .map_err(|e| BlockDBError::IoError(e))?;
        }

        // Remove collection data directory
        let collection_dir = format!("{}/collections/{}", self.config.data_dir, collection_id);
        if std::path::Path::new(&collection_dir).exists() {
            std::fs::remove_dir_all(&collection_dir)
                .map_err(|e| BlockDBError::IoError(e))?;
        }

        println!("✅ Collection '{}' dropped successfully", collection_id);
        Ok(())
    }

    pub fn get_collection(&self, collection_id: &str) -> Result<Collection, BlockDBError> {
        let collections = self.collections.read().unwrap();
        match collections.get(collection_id) {
            Some(collection) => {
                // Clone the collection for safe access
                let metadata = collection.metadata.read().unwrap().clone();
                let collection_clone = Collection::new(metadata, self.config.clone())?;
                Ok(collection_clone)
            }
            None => Err(BlockDBError::ApiError(format!(
                "Collection '{}' not found",
                collection_id
            ))),
        }
    }

    pub fn list_collections(&self) -> Result<Vec<CollectionMetadata>, BlockDBError> {
        let metadata_store = self.metadata_store.read().unwrap();
        Ok(metadata_store.values().cloned().collect())
    }

    pub fn collection_exists(&self, collection_id: &str) -> bool {
        let collections = self.collections.read().unwrap();
        collections.contains_key(collection_id)
    }

    pub fn get_collection_by_name(&self, name: &str) -> Result<Option<CollectionId>, BlockDBError> {
        let metadata_store = self.metadata_store.read().unwrap();
        for (id, metadata) in metadata_store.iter() {
            if metadata.name == name {
                return Ok(Some(id.clone()));
            }
        }
        Ok(None)
    }

    pub fn put(&self, collection_id: &str, key: &[u8], value: &[u8]) -> Result<(), BlockDBError> {
        let collections = self.collections.read().unwrap();
        match collections.get(collection_id) {
            Some(collection) => collection.put(key, value),
            None => Err(BlockDBError::ApiError(format!(
                "Collection '{}' not found",
                collection_id
            ))),
        }
    }

    pub fn get(&self, collection_id: &str, key: &[u8]) -> Result<Option<Vec<u8>>, BlockDBError> {
        let collections = self.collections.read().unwrap();
        match collections.get(collection_id) {
            Some(collection) => collection.get(key),
            None => Err(BlockDBError::ApiError(format!(
                "Collection '{}' not found",
                collection_id
            ))),
        }
    }

    pub fn delete(&self, collection_id: &str, key: &[u8]) -> Result<(), BlockDBError> {
        let collections = self.collections.read().unwrap();
        match collections.get(collection_id) {
            Some(collection) => collection.delete(key),
            None => Err(BlockDBError::ApiError(format!(
                "Collection '{}' not found",
                collection_id
            ))),
        }
    }

    pub fn list_keys(&self, collection_id: &str, prefix: Option<&[u8]>, limit: Option<usize>) -> Result<Vec<Vec<u8>>, BlockDBError> {
        let collections = self.collections.read().unwrap();
        match collections.get(collection_id) {
            Some(collection) => collection.list_keys(prefix, limit),
            None => Err(BlockDBError::ApiError(format!(
                "Collection '{}' not found",
                collection_id
            ))),
        }
    }

    pub fn get_collection_stats(&self, collection_id: &str) -> Result<CollectionStats, BlockDBError> {
        let collections = self.collections.read().unwrap();
        match collections.get(collection_id) {
            Some(collection) => collection.get_stats(),
            None => Err(BlockDBError::ApiError(format!(
                "Collection '{}' not found",
                collection_id
            ))),
        }
    }

    pub fn create_index(&self, collection_id: &str, index_def: IndexDefinition) -> Result<(), BlockDBError> {
        let collections = self.collections.read().unwrap();
        match collections.get(collection_id) {
            Some(collection) => collection.create_index(index_def),
            None => Err(BlockDBError::ApiError(format!(
                "Collection '{}' not found",
                collection_id
            ))),
        }
    }

    pub fn drop_index(&self, collection_id: &str, index_name: &str) -> Result<(), BlockDBError> {
        let collections = self.collections.read().unwrap();
        match collections.get(collection_id) {
            Some(collection) => collection.drop_index(index_name),
            None => Err(BlockDBError::ApiError(format!(
                "Collection '{}' not found",
                collection_id
            ))),
        }
    }

    pub fn verify_all_integrity(&self) -> Result<bool, BlockDBError> {
        let collections = self.collections.read().unwrap();
        for (collection_id, collection) in collections.iter() {
            match collection.verify_integrity() {
                Ok(true) => continue,
                Ok(false) => {
                    println!("❌ Integrity check failed for collection: {}", collection_id);
                    return Ok(false);
                }
                Err(e) => {
                    println!("❌ Error checking integrity for collection {}: {:?}", collection_id, e);
                    return Err(e);
                }
            }
        }
        println!("✅ Integrity verification passed for all collections");
        Ok(true)
    }

    pub fn get_total_stats(&self) -> Result<(usize, u64, u64), BlockDBError> {
        let collections = self.collections.read().unwrap();
        let mut total_collections = collections.len();
        let mut total_documents = 0u64;
        let mut total_size_bytes = 0u64;

        for collection in collections.values() {
            let stats = collection.get_stats()?;
            total_documents += stats.document_count;
            total_size_bytes += stats.total_size_bytes;
        }

        Ok((total_collections, total_documents, total_size_bytes))
    }

    fn load_existing_collections(&self) -> Result<(), BlockDBError> {
        let collections_dir = format!("{}/collections", self.config.data_dir);
        
        if !std::path::Path::new(&collections_dir).exists() {
            return Ok(());
        }

        let entries = std::fs::read_dir(&collections_dir)
            .map_err(|e| BlockDBError::IoError(e))?;

        for entry in entries {
            let entry = entry.map_err(|e| BlockDBError::IoError(e))?;
            let collection_id = entry.file_name().to_string_lossy().to_string();
            
            // Load metadata
            let metadata_path = format!("{}/{}/metadata.toml", collections_dir, collection_id);
            if std::path::Path::new(&metadata_path).exists() {
                match self.load_collection_metadata(&collection_id) {
                    Ok(metadata) => {
                        // Create collection instance
                        match Collection::new(metadata.clone(), self.config.clone()) {
                            Ok(collection) => {
                                let mut collections = self.collections.write().unwrap();
                                collections.insert(collection_id.clone(), collection);
                                
                                let mut metadata_store = self.metadata_store.write().unwrap();
                                metadata_store.insert(collection_id.clone(), metadata);
                                
                                println!("✅ Loaded existing collection: {}", collection_id);
                            }
                            Err(e) => {
                                println!("⚠️ Failed to load collection {}: {:?}", collection_id, e);
                            }
                        }
                    }
                    Err(e) => {
                        println!("⚠️ Failed to load metadata for collection {}: {:?}", collection_id, e);
                    }
                }
            }
        }

        Ok(())
    }

    fn persist_collection_metadata(&self, collection_id: &str) -> Result<(), BlockDBError> {
        let metadata = {
            let metadata_store = self.metadata_store.read().unwrap();
            metadata_store.get(collection_id).cloned()
        };

        if let Some(metadata) = metadata {
            let collection_dir = format!("{}/collections/{}", self.config.data_dir, collection_id);
            std::fs::create_dir_all(&collection_dir)
                .map_err(|e| BlockDBError::IoError(e))?;

            let metadata_path = format!("{}/metadata.toml", collection_dir);
            let metadata_toml = toml::to_string(&metadata)
                .map_err(|e| BlockDBError::ApiError(format!("Failed to serialize metadata: {}", e)))?;

            std::fs::write(&metadata_path, metadata_toml)
                .map_err(|e| BlockDBError::IoError(e))?;
        }

        Ok(())
    }

    fn load_collection_metadata(&self, collection_id: &str) -> Result<CollectionMetadata, BlockDBError> {
        let metadata_path = format!("{}/collections/{}/metadata.toml", self.config.data_dir, collection_id);
        let metadata_content = std::fs::read_to_string(&metadata_path)
            .map_err(|e| BlockDBError::IoError(e))?;

        let metadata: CollectionMetadata = toml::from_str(&metadata_content)
            .map_err(|e| BlockDBError::ApiError(format!("Failed to parse metadata: {}", e)))?;

        Ok(metadata)
    }

    /// Flush all data in a specific collection
    pub fn flush_collection(&self, collection_id: &str) -> Result<(), BlockDBError> {
        let collections = self.collections.read().unwrap();
        match collections.get(collection_id) {
            Some(collection) => {
                collection.flush()?;
                // Re-persist metadata after flush
                drop(collections);
                self.persist_collection_metadata(collection_id)?;
                println!("✅ Collection '{}' flushed successfully", collection_id);
                Ok(())
            }
            None => Err(BlockDBError::ApiError(format!(
                "Collection '{}' not found",
                collection_id
            ))),
        }
    }

    /// Flush all collections (entire node)
    pub fn flush_all(&self) -> Result<(), BlockDBError> {
        let collections = self.collections.read().unwrap();
        let collection_ids: Vec<String> = collections.keys().cloned().collect();
        drop(collections);
        
        for collection_id in collection_ids {
            self.flush_collection(&collection_id)?;
        }
        
        println!("✅ All collections flushed successfully");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_collection_metadata_creation() {
        let metadata = CollectionMetadata::new("users".to_string(), Some("admin".to_string()));
        
        assert_eq!(metadata.name, "users");
        assert!(metadata.id.starts_with("col_"));
        assert_eq!(metadata.created_by, Some("admin".to_string()));
        assert!(metadata.created_at > 0);
    }

    #[test]
    fn test_collection_creation() {
        let temp_dir = TempDir::new().unwrap();
        let metadata = CollectionMetadata::new("test_collection".to_string(), None);
        
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            memtable_size_limit: 1024 * 1024,
            wal_sync_interval_ms: 1000,
            compaction_threshold: 4,
            blockchain_batch_size: 100,
        };

        let collection = Collection::new(metadata, config).unwrap();
        
        // Test basic operations
        collection.put(b"key1", b"value1").unwrap();
        let value = collection.get(b"key1").unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));
        
        // Test stats
        let stats = collection.get_stats().unwrap();
        assert_eq!(stats.document_count, 1);
        assert!(stats.total_size_bytes > 0);
    }

    #[test]
    fn test_index_operations() {
        let temp_dir = TempDir::new().unwrap();
        let metadata = CollectionMetadata::new("indexed_collection".to_string(), None);
        
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            memtable_size_limit: 1024 * 1024,
            wal_sync_interval_ms: 1000,
            compaction_threshold: 4,
            blockchain_batch_size: 100,
        };

        let collection = Collection::new(metadata, config).unwrap();
        
        // Create index
        let index_def = IndexDefinition {
            name: "email_index".to_string(),
            fields: vec!["email".to_string()],
            unique: true,
            sparse: false,
        };
        
        collection.create_index(index_def).unwrap();
        
        // Add some data
        collection.put(b"user1", b"{'email': 'alice@example.com'}").unwrap();
        collection.put(b"user2", b"{'email': 'bob@example.com'}").unwrap();
        
        // Verify index exists
        let indexes = collection.indexes.read().unwrap();
        assert!(indexes.contains_key("email_index"));
        
        // Drop index
        collection.drop_index("email_index").unwrap();
        
        let indexes = collection.indexes.read().unwrap();
        assert!(!indexes.contains_key("email_index"));
    }

    #[test]
    fn test_collection_with_schema() {
        let mut schema = CollectionSchema {
            version: 1,
            fields: HashMap::new(),
            required_fields: vec!["name".to_string(), "email".to_string()],
            indexes: Vec::new(),
        };

        schema.fields.insert("name".to_string(), FieldDefinition {
            field_type: FieldType::String,
            required: true,
            default_value: None,
            validation_rules: vec![ValidationRule::MinLength(1), ValidationRule::MaxLength(100)],
        });

        schema.fields.insert("email".to_string(), FieldDefinition {
            field_type: FieldType::String,
            required: true,
            default_value: None,
            validation_rules: vec![ValidationRule::Pattern("^[^@]+@[^@]+\\.[^@]+$".to_string())],
        });

        let metadata = CollectionMetadata::new("users".to_string(), None)
            .with_schema(schema)
            .with_description("User data collection".to_string());

        assert!(metadata.schema.is_some());
        assert_eq!(metadata.description, Some("User data collection".to_string()));
        
        let schema = metadata.schema.unwrap();
        assert_eq!(schema.required_fields.len(), 2);
        assert_eq!(schema.fields.len(), 2);
    }

    #[test]
    fn test_collection_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            memtable_size_limit: 1024 * 1024,
            wal_sync_interval_ms: 1000,
            compaction_threshold: 4,
            blockchain_batch_size: 100,
        };

        let manager = CollectionManager::new(config).unwrap();
        
        // Should start with no collections
        let collections = manager.list_collections().unwrap();
        assert_eq!(collections.len(), 0);
        
        // Total stats should be empty
        let (total_collections, total_docs, total_size) = manager.get_total_stats().unwrap();
        assert_eq!(total_collections, 0);
        assert_eq!(total_docs, 0);
        assert_eq!(total_size, 0);
    }

    #[test]
    fn test_collection_manager_operations() {
        let temp_dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            memtable_size_limit: 1024 * 1024,
            wal_sync_interval_ms: 1000,
            compaction_threshold: 4,
            blockchain_batch_size: 100,
        };

        let manager = CollectionManager::new(config).unwrap();
        
        // Create collections
        let users_id = manager.create_collection(
            "users".to_string(), 
            None, 
            None, 
            Some("admin".to_string())
        ).unwrap();
        
        let orders_id = manager.create_collection(
            "orders".to_string(), 
            None, 
            None, 
            Some("admin".to_string())
        ).unwrap();
        
        // Verify collections exist
        assert!(manager.collection_exists(&users_id));
        assert!(manager.collection_exists(&orders_id));
        assert!(!manager.collection_exists("nonexistent"));
        
        // Test collection operations
        manager.put(&users_id, b"user1", b"Alice").unwrap();
        manager.put(&users_id, b"user2", b"Bob").unwrap();
        manager.put(&orders_id, b"order1", b"Product A").unwrap();
        
        // Test retrieval
        assert_eq!(manager.get(&users_id, b"user1").unwrap(), Some(b"Alice".to_vec()));
        assert_eq!(manager.get(&orders_id, b"order1").unwrap(), Some(b"Product A".to_vec()));
        
        // Test isolation between collections
        assert_eq!(manager.get(&users_id, b"order1").unwrap(), None);
        assert_eq!(manager.get(&orders_id, b"user1").unwrap(), None);
        
        // List collections
        let collections = manager.list_collections().unwrap();
        assert_eq!(collections.len(), 2);
        
        // Get collection by name
        let found_users_id = manager.get_collection_by_name("users").unwrap();
        assert_eq!(found_users_id, Some(users_id.clone()));
        
        let not_found = manager.get_collection_by_name("nonexistent").unwrap();
        assert_eq!(not_found, None);
        
        // Get collection stats
        let users_stats = manager.get_collection_stats(&users_id).unwrap();
        assert_eq!(users_stats.document_count, 2);
        
        let orders_stats = manager.get_collection_stats(&orders_id).unwrap();
        assert_eq!(orders_stats.document_count, 1);
        
        // Total stats
        let (total_collections, total_docs, total_size) = manager.get_total_stats().unwrap();
        assert_eq!(total_collections, 2);
        assert_eq!(total_docs, 3);
        assert!(total_size > 0);
        
        // Verify integrity
        assert!(manager.verify_all_integrity().unwrap());
    }

    #[test]
    fn test_collection_manager_drop_collection() {
        let temp_dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            memtable_size_limit: 1024 * 1024,
            wal_sync_interval_ms: 1000,
            compaction_threshold: 4,
            blockchain_batch_size: 100,
        };

        let manager = CollectionManager::new(config).unwrap();
        
        // Create a collection
        let collection_id = manager.create_collection(
            "test_collection".to_string(), 
            None, 
            None, 
            None
        ).unwrap();
        
        // Add some data
        manager.put(&collection_id, b"key1", b"value1").unwrap();
        
        // Verify it exists
        assert!(manager.collection_exists(&collection_id));
        assert_eq!(manager.get(&collection_id, b"key1").unwrap(), Some(b"value1".to_vec()));
        
        // Drop the collection
        manager.drop_collection(&collection_id).unwrap();
        
        // Verify it no longer exists
        assert!(!manager.collection_exists(&collection_id));
        assert!(manager.get(&collection_id, b"key1").is_err());
        
        // Try to drop non-existent collection
        assert!(manager.drop_collection("nonexistent").is_err());
    }

    #[test]
    fn test_collection_manager_duplicate_names() {
        let temp_dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            memtable_size_limit: 1024 * 1024,
            wal_sync_interval_ms: 1000,
            compaction_threshold: 4,
            blockchain_batch_size: 100,
        };

        let manager = CollectionManager::new(config).unwrap();
        
        // Create first collection
        let collection1_id = manager.create_collection(
            "users".to_string(), 
            None, 
            None, 
            None
        ).unwrap();
        
        // Try to create another collection with same name - should fail
        let result = manager.create_collection(
            "users".to_string(), 
            None, 
            None, 
            None
        );
        assert!(result.is_err());
        
        // Verify only one collection exists
        let collections = manager.list_collections().unwrap();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].name, "users");
    }

    #[test]
    fn test_collection_manager_with_schema_and_indexes() {
        let temp_dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            memtable_size_limit: 1024 * 1024,
            wal_sync_interval_ms: 1000,
            compaction_threshold: 4,
            blockchain_batch_size: 100,
        };

        let manager = CollectionManager::new(config).unwrap();
        
        // Create schema
        let mut schema = CollectionSchema {
            version: 1,
            fields: HashMap::new(),
            required_fields: vec!["email".to_string()],
            indexes: Vec::new(),
        };
        
        schema.fields.insert("email".to_string(), FieldDefinition {
            field_type: FieldType::String,
            required: true,
            default_value: None,
            validation_rules: vec![ValidationRule::Pattern("^[^@]+@[^@]+\\.[^@]+$".to_string())],
        });
        
        // Create collection with schema
        let collection_id = manager.create_collection(
            "users".to_string(), 
            Some(schema), 
            None, 
            None
        ).unwrap();
        
        // Create index
        let index_def = IndexDefinition {
            name: "email_index".to_string(),
            fields: vec!["email".to_string()],
            unique: true,
            sparse: false,
        };
        
        manager.create_index(&collection_id, index_def).unwrap();
        
        // Add data
        manager.put(&collection_id, b"user1", b"alice@example.com").unwrap();
        
        // Verify data
        assert_eq!(manager.get(&collection_id, b"user1").unwrap(), Some(b"alice@example.com".to_vec()));
        
        // Drop index
        manager.drop_index(&collection_id, "email_index").unwrap();
    }
}