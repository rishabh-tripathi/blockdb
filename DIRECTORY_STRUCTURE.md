# 📁 BlockDB Directory Structure

This document outlines the reorganized directory structure for BlockDB, providing clear organization for source code, documentation, examples, and deployment configurations.

## 🗂️ Root Directory Structure

```
blockdb/
├── 📋 Core Project Files
│   ├── README.md                    # Main project documentation
│   ├── LICENSE                      # MIT license
│   ├── CONTRIBUTING.md              # Contribution guidelines
│   ├── Cargo.toml                   # Rust project configuration
│   ├── Cargo.lock                   # Dependency lock file
│   └── DIRECTORY_STRUCTURE.md       # This file
│
├── 📁 src/                          # Source code
│   ├── lib.rs                       # Library entry point
│   ├── error.rs                     # Error types and handling
│   ├── distributed.rs               # Distributed database interface
│   ├── api/                         # API layer
│   ├── auth/                        # Authentication system
│   ├── consensus/                   # Raft consensus implementation
│   ├── storage/                     # Storage engine
│   ├── transaction/                 # Transaction management
│   └── bin/                         # Binary executables
│
├── 📁 docs/                         # Documentation
│   ├── README.md                    # Documentation index
│   ├── API_REFERENCE.md             # Complete API documentation
│   ├── ARCHITECTURE.md              # System architecture guide
│   ├── DEPLOYMENT.md                # Deployment instructions
│   ├── PERFORMANCE_TUNING.md        # Performance optimization
│   ├── TROUBLESHOOTING.md           # Issue resolution guide
│   ├── development/                 # Development documentation
│   ├── examples/                    # Example documentation
│   └── testing/                     # Test documentation
│
├── 📁 examples/                     # Code examples and demos
│   ├── usage/                       # Usage examples
│   └── demos/                       # Interactive demonstrations
│
├── 📁 tests/                        # Test suites
│   ├── integration/                 # Integration tests
│   └── performance/                 # Performance benchmarks
│
├── 📁 docker/                       # Docker configuration
│   ├── Dockerfile                   # Main Docker image
│   ├── docker-compose.yml           # Single node setup
│   ├── docker-compose.cluster.yml   # Multi-node cluster
│   ├── blockdb.toml                 # Container configuration
│   ├── entrypoint.sh                # Container entrypoint
│   └── nginx.conf                   # Load balancer config
│
├── 📁 k8s/                          # Kubernetes manifests
│   ├── namespace.yaml               # Namespace definition
│   ├── configmap.yaml               # Configuration maps
│   ├── statefulset.yaml             # StatefulSet deployment
│   ├── service.yaml                 # Service definitions
│   ├── ingress.yaml                 # Ingress configuration
│   ├── rbac.yaml                    # RBAC permissions
│   ├── pdb.yaml                     # Pod disruption budgets
│   └── kustomization.yaml           # Kustomization config
│
├── 📁 scripts/                      # Build and deployment scripts
│   ├── docker-build.sh              # Docker build script
│   ├── docker-deploy.sh             # Docker deployment
│   └── k8s-deploy.sh                # Kubernetes deployment
│
├── 📁 blockdb_data/                 # Runtime data (gitignored)
│   ├── wal.log                      # Write-ahead log
│   ├── blockchain.dat               # Blockchain data
│   └── collections/                 # Collection data
│
└── 📁 target/                       # Build artifacts (gitignored)
    ├── debug/                       # Debug builds
    └── release/                     # Release builds
```

## 📁 Detailed Directory Breakdown

### `/src/` - Source Code
```
src/
├── lib.rs                           # Library entry point and public API
├── error.rs                         # Error types and handling
├── distributed.rs                   # Main distributed database interface
├── api/
│   └── mod.rs                       # HTTP and CLI API implementations
├── auth/
│   ├── mod.rs                       # Authentication module interface
│   ├── auth_manager.rs              # Authentication manager
│   ├── crypto.rs                    # Cryptographic utilities
│   ├── distributed_auth.rs          # Distributed authentication
│   ├── identity.rs                  # Identity management
│   ├── permissions.rs               # Permission system
│   ├── simple_tests.rs              # Simple authentication tests
│   └── tests.rs                     # Comprehensive auth tests
├── consensus/
│   ├── mod.rs                       # Consensus module interface
│   ├── raft.rs                      # Raft algorithm implementation
│   └── log_entry.rs                 # Replication log structures
├── storage/
│   ├── mod.rs                       # Storage engine interface
│   ├── memtable.rs                  # In-memory sorted table
│   ├── sstable.rs                   # Immutable disk-based tables
│   ├── wal.rs                       # Write-ahead logging
│   ├── blockchain.rs                # Cryptographic verification
│   ├── collection.rs                # Collection management system
│   └── compaction.rs                # Background SSTable merging
├── transaction/
│   ├── mod.rs                       # Transaction management interface
│   ├── lock_manager.rs              # Distributed locking system
│   └── transaction_log.rs           # Transaction state tracking
└── bin/
    ├── blockdb-server.rs             # Main server binary
    └── blockdb-cli.rs                # Command-line interface
```

### `/docs/` - Documentation
```
docs/
├── README.md                        # Documentation overview
├── API_REFERENCE.md                 # Complete API documentation
├── ARCHITECTURE.md                  # System architecture and design
├── DEPLOYMENT.md                    # Production deployment guide
├── PERFORMANCE_TUNING.md            # Performance optimization guide
├── TROUBLESHOOTING.md               # Issue diagnosis and resolution
├── development/
│   ├── AI_DEVELOPMENT.md            # AI development workflows
│   ├── CLAUDE.md                    # Project context for AI/LLM
│   └── COLLECTION_SYSTEM.md         # Collection system documentation
├── examples/
│   └── DEMO.md                      # Usage demonstrations
└── testing/
    ├── TEST_REPORT.md               # Test execution reports
    └── TEST_RESULTS.md              # Test results and analysis
```

### `/examples/` - Code Examples
```
examples/
├── usage/
│   ├── basic_usage.rs               # Basic database operations
│   └── USAGE_EXAMPLE.rs             # Comprehensive usage patterns
└── demos/
    ├── simple_demo.rs               # Simple database demonstration
    ├── collection_demo.rs           # Collection system demonstration
    └── collection_flow_test.rs      # Collection flow testing
```

### `/tests/` - Test Suites
```
tests/
├── integration/
│   ├── test_blockdb.py              # Basic database integration tests
│   ├── test_acid_properties.py     # ACID compliance tests
│   └── test_distributed_features.py # Distributed system tests
└── performance/
    └── performance_test.rs          # Performance benchmarks
```

### `/docker/` - Container Configuration
```
docker/
├── Dockerfile                       # Multi-stage Docker build
├── docker-compose.yml               # Single node development
├── docker-compose.cluster.yml       # Multi-node cluster setup
├── blockdb.toml                     # Container configuration
├── entrypoint.sh                    # Container startup script
└── nginx.conf                       # Load balancer configuration
```

### `/k8s/` - Kubernetes Manifests
```
k8s/
├── namespace.yaml                   # Kubernetes namespace
├── configmap.yaml                   # Configuration data
├── statefulset.yaml                 # StatefulSet for persistent storage
├── service.yaml                     # Service definitions
├── ingress.yaml                     # Ingress routing
├── rbac.yaml                        # Role-based access control
├── pdb.yaml                         # Pod disruption budgets
└── kustomization.yaml               # Kustomization configuration
```

### `/scripts/` - Automation Scripts
```
scripts/
├── docker-build.sh                  # Docker image build automation
├── docker-deploy.sh                 # Docker deployment automation
└── k8s-deploy.sh                    # Kubernetes deployment automation
```

## 🎯 Organization Principles

### 1. **Separation of Concerns**
- **Source code** (`src/`) - Implementation only
- **Documentation** (`docs/`) - All documentation centralized
- **Examples** (`examples/`) - Code demonstrations
- **Tests** (`tests/`) - All test suites
- **Deployment** (`docker/`, `k8s/`) - Infrastructure configuration

### 2. **Logical Grouping**
- **By functionality** within `src/` (storage, consensus, auth, etc.)
- **By audience** within `docs/` (development, examples, testing)
- **By type** within `examples/` (usage, demos)
- **By scope** within `tests/` (integration, performance)

### 3. **Clear Naming Conventions**
- **Descriptive directories** that indicate purpose
- **Consistent file naming** across similar components
- **Hierarchical organization** from general to specific

### 4. **Development Workflow Support**
- **Easy navigation** for developers
- **Clear documentation paths** for users
- **Logical test organization** for CI/CD
- **Deployment separation** for DevOps

## 🔍 Finding Components

### **For Developers:**
- **Core logic**: `src/storage/`, `src/consensus/`, `src/distributed.rs`
- **API interfaces**: `src/api/`, `src/bin/`
- **Authentication**: `src/auth/`
- **Transaction handling**: `src/transaction/`

### **For Users:**
- **Getting started**: `README.md`, `docs/examples/DEMO.md`
- **API reference**: `docs/API_REFERENCE.md`
- **Deployment**: `docs/DEPLOYMENT.md`, `docker/`, `k8s/`
- **Troubleshooting**: `docs/TROUBLESHOOTING.md`

### **For Contributors:**
- **Development setup**: `CONTRIBUTING.md`, `docs/development/`
- **Architecture**: `docs/ARCHITECTURE.md`
- **Testing**: `tests/`, `docs/testing/`
- **Examples**: `examples/`

### **For DevOps:**
- **Container deployment**: `docker/`
- **Kubernetes deployment**: `k8s/`
- **Automation scripts**: `scripts/`
- **Configuration**: `*.toml`, `*.yaml`

## 📊 Directory Statistics

| Directory | Files | Purpose | Audience |
|-----------|-------|---------|----------|
| `src/` | 20+ | Source code | Developers |
| `docs/` | 9 | Documentation | Users, Contributors |
| `examples/` | 4 | Code examples | Users, Developers |
| `tests/` | 4 | Test suites | Developers, QA |
| `docker/` | 6 | Container config | DevOps |
| `k8s/` | 8 | Kubernetes config | DevOps |
| `scripts/` | 3 | Automation | DevOps |

## 🔄 Maintenance Guidelines

### **Adding New Components:**
1. **Source code** → `src/` with appropriate module
2. **Documentation** → `docs/` with clear categorization
3. **Examples** → `examples/` with usage demonstrations
4. **Tests** → `tests/` with appropriate test type

### **File Naming:**
- **Source files**: `snake_case.rs`
- **Documentation**: `UPPER_CASE.md`
- **Examples**: `descriptive_name.rs`
- **Tests**: `test_functionality.rs` or `test_functionality.py`

### **Documentation Updates:**
- **API changes** → Update `docs/API_REFERENCE.md`
- **Architecture changes** → Update `docs/ARCHITECTURE.md`
- **New features** → Update `README.md` and relevant docs
- **Deployment changes** → Update `docs/DEPLOYMENT.md`

This reorganized structure provides clear separation of concerns, logical grouping, and easy navigation for all stakeholders while maintaining the project's comprehensive feature set.