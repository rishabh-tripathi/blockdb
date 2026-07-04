//! Integration tests for the authentication manager using its real API
//! (Argon2-hashed identities, sessions, permissions, account lockout, audit trail).

use blockdb::auth::{AuthConfig, PermissionSet};
use blockdb::{AuthManager, Permission};

fn manager() -> AuthManager {
    AuthManager::new(AuthConfig {
        enabled: true,
        session_duration_hours: 1,
        max_failed_attempts: 3,
        password_min_length: 6,
        require_strong_passwords: false,
        admin_users: vec![],
        allow_anonymous_reads: false,
        token_refresh_threshold_hours: 1,
    })
}

#[tokio::test]
async fn test_create_authenticate_and_session_flow() {
    let am = manager();

    am.create_user(
        "testuser".to_string(),
        "testpass123",
        PermissionSet::from_permissions(vec![Permission::Read, Permission::Write]),
        Some("admin".to_string()),
        0,
    )
    .unwrap();

    let ctx = am.authenticate("testuser", "testpass123").unwrap();
    assert!(ctx.has_permission(&Permission::Read));
    assert!(ctx.has_permission(&Permission::Write));
    assert!(!ctx.has_permission(&Permission::Admin));

    // Token validates while the session is live.
    let validated = am.authenticate_with_token(&ctx.session_id).unwrap();
    assert_eq!(validated.user_id, "testuser");

    // Logout invalidates the session.
    am.logout(&ctx.session_id).unwrap();
    assert!(am.authenticate_with_token(&ctx.session_id).is_err());
}

#[tokio::test]
async fn test_duplicate_user_rejected() {
    let am = manager();
    am.create_user("dup".to_string(), "password1", PermissionSet::read_only(), None, 0)
        .unwrap();
    let second = am.create_user("dup".to_string(), "password2", PermissionSet::read_only(), None, 0);
    assert!(second.is_err());
}

#[tokio::test]
async fn test_short_password_rejected() {
    let am = manager();
    let result = am.create_user("shorty".to_string(), "123", PermissionSet::read_only(), None, 0);
    assert!(result.is_err(), "password below min length must be rejected");
}

#[tokio::test]
async fn test_wrong_password_and_account_lockout() {
    let am = manager();
    am.create_user("locky".to_string(), "correct123", PermissionSet::read_only(), None, 0)
        .unwrap();

    // Three failed attempts (max_failed_attempts = 3) should lock the account.
    for _ in 0..3 {
        assert!(am.authenticate("locky", "wrong").is_err());
    }
    // Even the correct password is now refused because the account is locked.
    assert!(am.authenticate("locky", "correct123").is_err());
}

#[tokio::test]
async fn test_nonexistent_user_authentication_fails() {
    let am = manager();
    assert!(am.authenticate("ghost", "whatever").is_err());
}

#[tokio::test]
async fn test_permission_grant_and_revoke_with_audit() {
    let am = manager();

    // An admin who can grant/revoke.
    am.create_user("admin".to_string(), "adminpass1", PermissionSet::admin(), None, 0)
        .unwrap();
    am.create_user(
        "member".to_string(),
        "memberpass1",
        PermissionSet::read_only(),
        Some("admin".to_string()),
        1,
    )
    .unwrap();

    // Grant Write to the member.
    am.grant_permission("member", Permission::Write, "admin", 2).unwrap();
    assert!(am.check_permission("member", &Permission::Write).unwrap());

    // Revoke it again.
    am.revoke_permission("member", &Permission::Write, "admin", 3).unwrap();
    assert!(am.check_permission("member", &Permission::Write).is_err());

    // Both operations are recorded in the audit trail.
    let audit = am.get_audit_trail();
    assert!(audit.len() >= 2);

    // The identity chain remains internally consistent.
    assert!(am.verify_identity_chain_integrity());
}

#[tokio::test]
async fn test_admin_permission_implies_others() {
    let am = manager();
    am.create_user("root".to_string(), "rootpass1", PermissionSet::admin(), None, 0)
        .unwrap();
    let ctx = am.authenticate("root", "rootpass1").unwrap();
    assert!(ctx.has_permission(&Permission::Admin));
    // check_permission honours Admin implying everything.
    assert!(am.check_permission("root", &Permission::Write).unwrap());
    assert!(am.check_permission("root", &Permission::CreateUser).unwrap());
}
