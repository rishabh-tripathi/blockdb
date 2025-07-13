# Contributing to BlockDB

We welcome contributions to BlockDB! This document provides guidelines for contributing to the project.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Workflow](#development-workflow)
- [Code Standards](#code-standards)
- [Testing Guidelines](#testing-guidelines)
- [Documentation](#documentation)
- [Submitting Changes](#submitting-changes)
- [Review Process](#review-process)

## Code of Conduct

This project adheres to the Contributor Covenant [code of conduct](CODE_OF_CONDUCT.md). By participating, you are expected to uphold this code.

## Getting Started

### Prerequisites

- **Rust**: 1.70.0 or later
- **Git**: Latest stable version
- **Basic knowledge**: Rust, databases, distributed systems

### Development Setup

```bash
# Fork the repository on GitHub
# Clone your fork
git clone https://github.com/yourusername/blockdb.git
cd blockdb

# Add upstream remote
git remote add upstream https://github.com/username/blockdb.git

# Install development dependencies
cargo install cargo-watch cargo-audit cargo-tarpaulin

# Build and test
cargo build
cargo test
```

### Development Tools

```bash
# Auto-rebuild on changes
cargo watch -x check -x test

# Code formatting
cargo fmt

# Linting
cargo clippy

# Security audit
cargo audit

# Test coverage
cargo tarpaulin --out Html
```

## Development Workflow

### Branch Strategy

- **main**: Stable release branch
- **develop**: Integration branch for next release
- **feature/**: Feature development branches
- **fix/**: Bug fix branches
- **docs/**: Documentation updates

### Creating a Feature Branch

```bash
# Update your local main branch
git checkout main
git pull upstream main

# Create and switch to feature branch
git checkout -b feature/your-feature-name

# Work on your feature
# ... make changes ...

# Commit changes
git add .
git commit -m "Add your feature description"

# Push to your fork
git push origin feature/your-feature-name
```

### Keeping Your Branch Updated

```bash
# Regularly sync with upstream
git checkout main
git pull upstream main
git checkout feature/your-feature-name
git rebase main

# Resolve conflicts if any
# Force push after rebase (if needed)
git push origin feature/your-feature-name --force-with-lease
```

## Code Standards

### Rust Style Guidelines

Follow the official [Rust Style Guidelines](https://doc.rust-lang.org/1.0.0/style/):

```rust
// Good: Use snake_case for variables and functions
fn process_user_data(user_id: u64) -> Result<UserData, Error> {
    // ...
}

// Good: Use PascalCase for types
struct UserAccount {
    account_id: u64,
    username: String,
}

// Good: Use SCREAMING_SNAKE_CASE for constants
const MAX_RETRY_ATTEMPTS: u32 = 3;
```

### Code Organization

```rust
// File structure example: src/storage/mod.rs
pub mod memtable;
pub mod sstable;
pub mod wal;
pub mod blockchain;

pub use memtable::MemTable;
pub use sstable::SSTable;
pub use wal::WriteAheadLog;
pub use blockchain::Blockchain;

// Public interface
pub struct StorageEngine {
    // Internal implementation
}

impl StorageEngine {
    // Public methods first
    pub fn new() -> Self { /* ... */ }
    pub fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), Error> { /* ... */ }
    
    // Private methods last
    fn internal_method(&self) { /* ... */ }
}
```

### Error Handling

```rust
// Use custom error types
#[derive(Debug, thiserror::Error)]
pub enum BlockDBError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Storage error: {message}")]
    Storage { message: String },
    
    #[error("Consensus error: {0}")]
    Consensus(String),
}

// Proper error propagation
fn example_function() -> Result<(), BlockDBError> {
    let data = std::fs::read("file.txt")?; // Automatic conversion
    
    if data.is_empty() {
        return Err(BlockDBError::Storage {
            message: "File is empty".to_string()
        });
    }
    
    Ok(())
}
```

### Documentation Standards

```rust
/// Brief description of the function
///
/// Longer description with more details about what this function does,
/// its behavior, and any important considerations.
///
/// # Arguments
///
/// * `key` - The key to store (must be non-empty)
/// * `value` - The value to associate with the key
///
/// # Returns
///
/// Returns `Ok(())` on success, or an error if the operation fails.
///
/// # Errors
///
/// * `BlockDBError::DuplicateKey` - If the key already exists
/// * `BlockDBError::Io` - If there's an I/O error
///
/// # Examples
///
/// ```rust
/// let mut db = BlockDB::new(config)?;
/// db.put(b"user:123", b"John Doe")?;
/// ```
pub fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), BlockDBError> {
    // Implementation
}
```

### Performance Guidelines

```rust
// Good: Minimize allocations
fn process_keys(keys: &[&[u8]]) -> Vec<String> {
    keys.iter()
        .map(|key| String::from_utf8_lossy(key).into_owned())
        .collect()
}

// Good: Use appropriate data structures
use std::collections::HashMap;  // For key-value lookups
use std::collections::BTreeMap; // For ordered data
use std::collections::HashSet;  // For unique items

// Good: Avoid unnecessary clones
fn process_data(data: &Data) -> ProcessedData {
    // Work with references when possible
    ProcessedData::new(data.field1, &data.field2)
}
```

## Testing Guidelines

### Test Organization

```
tests/
├── unit/           # Unit tests for individual components
├── integration/    # Integration tests for combined functionality
├── performance/    # Performance benchmarks
└── fixtures/       # Test data and utilities
```

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_put_and_get() {
        let temp_dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        
        let mut db = BlockDB::new(config).unwrap();
        
        // Test successful put
        assert!(db.put(b"key1", b"value1").is_ok());
        
        // Test successful get
        let result = db.get(b"key1").unwrap();
        assert_eq!(result, Some(b"value1".to_vec()));
        
        // Test non-existent key
        let result = db.get(b"nonexistent").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_duplicate_key_error() {
        let temp_dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        
        let mut db = BlockDB::new(config).unwrap();
        
        // First put should succeed
        assert!(db.put(b"key1", b"value1").is_ok());
        
        // Second put should fail with duplicate key error
        let result = db.put(b"key1", b"value2");
        assert!(matches!(result, Err(BlockDBError::DuplicateKey(_))));
    }
}
```

### Integration Tests

```rust
// tests/integration/distributed_test.rs
use blockdb::{DistributedBlockDB, DistributedBlockDBConfig};
use tempfile::TempDir;

#[tokio::test]
async fn test_distributed_consensus() {
    // Setup multiple nodes
    let nodes = setup_test_cluster(3).await;
    
    // Test distributed operations
    let leader = &nodes[0];
    leader.put(b"distributed_key", b"distributed_value").await.unwrap();
    
    // Verify replication
    for node in &nodes {
        let result = node.get(b"distributed_key").await.unwrap();
        assert_eq!(result, Some(b"distributed_value".to_vec()));
    }
    
    // Cleanup
    cleanup_test_cluster(nodes).await;
}

async fn setup_test_cluster(size: usize) -> Vec<DistributedBlockDB> {
    // Implementation
}
```

### Performance Tests

```rust
// tests/performance/throughput_test.rs
use criterion::{criterion_group, criterion_main, Criterion};

fn benchmark_write_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_throughput");
    
    group.bench_function("single_thread", |b| {
        let temp_dir = TempDir::new().unwrap();
        let mut db = setup_test_db(&temp_dir);
        
        let mut counter = 0;
        b.iter(|| {
            let key = format!("key_{}", counter);
            let value = format!("value_{}", counter);
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
            counter += 1;
        });
    });
    
    group.finish();
}

criterion_group!(benches, benchmark_write_throughput);
criterion_main!(benches);
```

### Test Coverage

```bash
# Generate coverage report
cargo tarpaulin --out Html --output-dir coverage

# View coverage
open coverage/tarpaulin-report.html

# Aim for >80% coverage for new code
```

## Documentation

### Code Documentation

- **All public APIs** must have documentation
- **Complex algorithms** should be explained
- **Examples** should be provided for main functionality
- **Error conditions** should be documented

### User Documentation

- **README.md**: Project overview and quick start
- **API_REFERENCE.md**: Complete API documentation
- **ARCHITECTURE.md**: System design and components
- **DEPLOYMENT.md**: Installation and deployment guides
- **TROUBLESHOOTING.md**: Common issues and solutions

### Contributing Documentation

```bash
# Build documentation locally
cargo doc --no-deps --open

# Check for broken links
cargo doc --no-deps 2>&1 | grep warning
```

## Submitting Changes

### Pre-submission Checklist

- [ ] Code compiles without warnings
- [ ] All tests pass
- [ ] Code follows style guidelines
- [ ] Documentation is updated
- [ ] Performance impact is acceptable
- [ ] Security implications are considered

### Commit Message Format

```
type(scope): brief description

Longer description explaining the change in detail.
Include the motivation for the change and how it works.

Closes #123
```

**Types:**
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes (formatting, etc.)
- `refactor`: Code refactoring
- `test`: Adding or updating tests
- `perf`: Performance improvements
- `chore`: Build process or auxiliary tool changes

**Examples:**
```
feat(storage): add LSM-tree compaction

Implement background compaction for LSM-tree storage engine.
This reduces read amplification and improves query performance.

- Add compaction trigger based on SSTable count
- Implement level-based compaction strategy
- Add compaction metrics and monitoring

Closes #45

fix(consensus): handle network partition correctly

Fix issue where nodes could get stuck during network partitions.
The election timeout was too aggressive for high-latency networks.

- Increase default election timeout to 300ms
- Add exponential backoff for retries
- Improve partition detection logic

Fixes #78
```

### Pull Request Process

1. **Create PR** from your feature branch to `develop`
2. **Fill out PR template** with description and checklist
3. **Request review** from maintainers
4. **Address feedback** and update PR as needed
5. **Squash commits** if requested
6. **Wait for approval** and merge

### PR Template

```markdown
## Description
Brief description of the change and which issue it fixes.

## Type of Change
- [ ] Bug fix (non-breaking change which fixes an issue)
- [ ] New feature (non-breaking change which adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to not work as expected)
- [ ] Documentation update

## Testing
- [ ] Unit tests added/updated
- [ ] Integration tests added/updated
- [ ] Performance tests added/updated
- [ ] Manual testing completed

## Documentation
- [ ] Code comments updated
- [ ] API documentation updated
- [ ] User documentation updated
- [ ] Migration guide provided (if breaking change)

## Performance Impact
- [ ] No performance impact
- [ ] Performance improvement
- [ ] Acceptable performance decrease
- [ ] Performance benchmarks included

## Security Considerations
- [ ] No security impact
- [ ] Security review completed
- [ ] Security documentation updated

## Checklist
- [ ] Code compiles without warnings
- [ ] All tests pass
- [ ] Code follows style guidelines
- [ ] Self-review completed
- [ ] Reviewers assigned
```

## Review Process

### For Contributors

- **Be responsive** to feedback
- **Explain your decisions** when asked
- **Keep discussions constructive**
- **Update documentation** as needed
- **Test thoroughly** before requesting review

### For Reviewers

- **Be respectful** and constructive
- **Focus on the code**, not the person
- **Explain reasoning** for requested changes
- **Approve when ready**, don't nitpick
- **Check for security** and performance implications

### Review Criteria

- **Correctness**: Does the code work as intended?
- **Performance**: Is the performance acceptable?
- **Security**: Are there any security vulnerabilities?
- **Maintainability**: Is the code easy to understand and maintain?
- **Testing**: Is the code adequately tested?
- **Documentation**: Is the code properly documented?

## Release Process

### Version Numbers

We follow [Semantic Versioning](https://semver.org/):
- **MAJOR**: Breaking changes
- **MINOR**: New features (backward compatible)
- **PATCH**: Bug fixes (backward compatible)

### Release Checklist

- [ ] All tests pass
- [ ] Documentation updated
- [ ] Changelog updated
- [ ] Version number bumped
- [ ] Security review completed
- [ ] Performance regression tests pass
- [ ] Migration guide provided (if needed)

## Community

### Communication Channels

- **GitHub Issues**: Bug reports and feature requests
- **GitHub Discussions**: General questions and discussions
- **Discord**: Real-time chat and support
- **Email**: Private security reports

### Getting Help

- **Documentation**: Check the docs first
- **Search Issues**: Look for existing solutions
- **Ask Questions**: Use GitHub Discussions
- **Report Bugs**: Use GitHub Issues with template

### Recognition

Contributors will be recognized in:
- **CONTRIBUTORS.md**: List of all contributors
- **Release Notes**: Major contributors for each release
- **Blog Posts**: Featured contributions

Thank you for contributing to BlockDB!