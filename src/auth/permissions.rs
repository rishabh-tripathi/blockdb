use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use crate::auth::{AuthError, UserId};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Permission {
    // Data operations
    Read,
    Write,
    Delete,
    
    // Transaction operations
    BeginTransaction,
    CommitTransaction,
    RollbackTransaction,
    
    // Administrative operations
    CreateUser,
    ModifyUser,
    DeleteUser,
    GrantPermission,
    RevokePermission,
    
    // System operations
    ViewStats,
    ViewHealth,
    ViewClusterStatus,
    ModifyClusterConfig,
    
    // Blockchain operations
    ViewBlockchain,
    VerifyIntegrity,
    
    // Administrative superuser
    Admin,
}

impl Permission {
    pub fn implies(&self, other: &Permission) -> bool {
        match self {
            Permission::Admin => true, // Admin implies all permissions
            Permission::Write => matches!(other, Permission::Read), // Write implies Read
            Permission::ModifyUser => matches!(other, Permission::CreateUser), // Modify implies Create
            Permission::GrantPermission => matches!(other, Permission::RevokePermission), // Grant implies Revoke
            _ => self == other,
        }
    }

    pub fn is_admin_only(&self) -> bool {
        matches!(
            self,
            Permission::CreateUser
                | Permission::ModifyUser
                | Permission::DeleteUser
                | Permission::GrantPermission
                | Permission::RevokePermission
                | Permission::ModifyClusterConfig
                | Permission::Admin
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionSet {
    permissions: HashSet<Permission>,
}

impl PermissionSet {
    pub fn new() -> Self {
        Self {
            permissions: HashSet::new(),
        }
    }

    pub fn from_permissions(permissions: Vec<Permission>) -> Self {
        Self {
            permissions: permissions.into_iter().collect(),
        }
    }

    pub fn admin() -> Self {
        Self {
            permissions: [Permission::Admin].into_iter().collect(),
        }
    }

    pub fn read_only() -> Self {
        Self {
            permissions: [Permission::Read, Permission::ViewStats, Permission::ViewHealth]
                .into_iter()
                .collect(),
        }
    }

    pub fn read_write() -> Self {
        Self {
            permissions: [
                Permission::Read,
                Permission::Write,
                Permission::BeginTransaction,
                Permission::CommitTransaction,
                Permission::RollbackTransaction,
                Permission::ViewStats,
                Permission::ViewHealth,
            ]
            .into_iter()
            .collect(),
        }
    }

    pub fn has_permission(&self, required: &Permission) -> bool {
        // Check if user has the exact permission or any permission that implies it
        self.permissions.iter().any(|p| p.implies(required))
    }

    pub fn add_permission(&mut self, permission: Permission) {
        self.permissions.insert(permission);
    }

    pub fn remove_permission(&mut self, permission: &Permission) {
        self.permissions.remove(permission);
    }

    pub fn get_permissions(&self) -> Vec<Permission> {
        self.permissions.iter().cloned().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.permissions.is_empty()
    }

    pub fn merge(&mut self, other: &PermissionSet) {
        self.permissions.extend(other.permissions.iter().cloned());
    }
}

impl Default for PermissionSet {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PermissionOperation {
    Grant {
        target_user: UserId,
        permission: Permission,
        granted_by: UserId,
        block_index: u64,
        timestamp: u64,
    },
    Revoke {
        target_user: UserId,
        permission: Permission,
        revoked_by: UserId,
        block_index: u64,
        timestamp: u64,
    },
    CreateRole {
        role_name: String,
        permissions: Vec<Permission>,
        created_by: UserId,
        block_index: u64,
        timestamp: u64,
    },
    AssignRole {
        target_user: UserId,
        role_name: String,
        assigned_by: UserId,
        block_index: u64,
        timestamp: u64,
    },
}

impl PermissionOperation {
    pub fn get_block_index(&self) -> u64 {
        match self {
            PermissionOperation::Grant { block_index, .. } => *block_index,
            PermissionOperation::Revoke { block_index, .. } => *block_index,
            PermissionOperation::CreateRole { block_index, .. } => *block_index,
            PermissionOperation::AssignRole { block_index, .. } => *block_index,
        }
    }

    pub fn get_timestamp(&self) -> u64 {
        match self {
            PermissionOperation::Grant { timestamp, .. } => *timestamp,
            PermissionOperation::Revoke { timestamp, .. } => *timestamp,
            PermissionOperation::CreateRole { timestamp, .. } => *timestamp,
            PermissionOperation::AssignRole { timestamp, .. } => *timestamp,
        }
    }

    pub fn get_operator(&self) -> &UserId {
        match self {
            PermissionOperation::Grant { granted_by, .. } => granted_by,
            PermissionOperation::Revoke { revoked_by, .. } => revoked_by,
            PermissionOperation::CreateRole { created_by, .. } => created_by,
            PermissionOperation::AssignRole { assigned_by, .. } => assigned_by,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalPermission {
    pub permission: Permission,
    pub valid_from: u64,
    pub valid_until: u64,
    pub conditions: Vec<AccessCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccessCondition {
    TimeRange { start: u64, end: u64 },
    MaxOperations { limit: u64, current: u64 },
    IPWhitelist { allowed_ips: Vec<String> },
    RequireAdditionalAuth,
}

impl TemporalPermission {
    pub fn is_valid(&self, current_time: u64) -> bool {
        current_time >= self.valid_from && current_time <= self.valid_until
    }

    pub fn check_conditions(&self, context: &AccessContext) -> bool {
        self.conditions.iter().all(|condition| match condition {
            AccessCondition::TimeRange { start, end } => {
                context.timestamp >= *start && context.timestamp <= *end
            }
            AccessCondition::MaxOperations { limit, current } => current < limit,
            AccessCondition::IPWhitelist { allowed_ips } => {
                context.client_ip.as_ref().map_or(false, |ip| allowed_ips.contains(ip))
            }
            AccessCondition::RequireAdditionalAuth => context.additional_auth_verified,
        })
    }
}

#[derive(Debug, Clone)]
pub struct AccessContext {
    pub timestamp: u64,
    pub client_ip: Option<String>,
    pub additional_auth_verified: bool,
    pub operation_count: u64,
}

impl Default for AccessContext {
    fn default() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        Self {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            client_ip: None,
            additional_auth_verified: false,
            operation_count: 0,
        }
    }
}