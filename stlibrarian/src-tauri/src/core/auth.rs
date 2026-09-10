use crate::core::models::User;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use rand_core::{OsRng, RngCore};
use argon2::Argon2;
use log::info;
use rusqlite::{params, Connection, OptionalExtension};
use std::time::{SystemTime, UNIX_EPOCH};

/// Sessions last a month. Long enough that a home NAS user is not re-typing a
/// password every day, short enough that a stolen token expires.
const SESSION_TTL_SECS: i64 = 60 * 60 * 24 * 30;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

pub fn verify_password(password: &str, stored: &str) -> bool {
    match PasswordHash::new(stored) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

pub fn user_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
        .unwrap_or(0)
}

pub fn create_user(
    conn: &Connection,
    username: &str,
    password: &str,
    role: &str,
) -> Result<User, String> {
    let username = username.trim();
    if username.is_empty() {
        return Err("Username cannot be empty".into());
    }
    if password.len() < 8 {
        return Err("Password must be at least 8 characters".into());
    }
    let hash = hash_password(password)?;
    conn.execute(
        "INSERT INTO users (username, password_hash, role, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![username, hash, role, now()],
    )
    .map_err(|e| format!("Could not create user: {}", e))?;
    Ok(User {
        id: conn.last_insert_rowid(),
        username: username.to_string(),
        role: role.to_string(),
    })
}

pub struct Session {
    pub token: String,
    pub user: User,
}

/// Exchange a username and password for a session token.
///
/// Replaces the previous hard-coded `admin`/`admin` check that returned the
/// literal string "dev-token" to anyone who asked.
pub fn login(conn: &Connection, username: &str, password: &str) -> Option<Session> {
    let row: Option<(i64, String, String)> = conn
        .query_row(
            "SELECT id, password_hash, role FROM users WHERE username = ?1",
            params![username.trim()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .ok()
        .flatten();

    let (id, stored, role) = row?;
    if !verify_password(password, &stored) {
        return None;
    }

    let token = random_hex(32);
    conn.execute(
        "INSERT INTO sessions (token, user_id, expires_at) VALUES (?1, ?2, ?3)",
        params![token, id, now() + SESSION_TTL_SECS],
    )
    .ok()?;

    Some(Session {
        token,
        user: User {
            id,
            username: username.trim().to_string(),
            role,
        },
    })
}

pub fn verify_token(conn: &Connection, token: &str) -> Option<User> {
    conn.query_row(
        "SELECT u.id, u.username, u.role
         FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.token = ?1 AND s.expires_at > ?2",
        params![token, now()],
        |r| {
            Ok(User {
                id: r.get(0)?,
                username: r.get(1)?,
                role: r.get(2)?,
            })
        },
    )
    .optional()
    .ok()
    .flatten()
}

pub fn logout(conn: &Connection, token: &str) {
    conn.execute("DELETE FROM sessions WHERE token = ?1", params![token])
        .ok();
}

pub fn purge_expired_sessions(conn: &Connection) {
    conn.execute("DELETE FROM sessions WHERE expires_at <= ?1", params![now()])
        .ok();
}

/// On an empty user table, mint an admin with a generated password and hand it
/// back exactly once so the caller can print it.
///
/// A shipped default password is a shipped vulnerability -- especially for the
/// Pro build, whose entire point is being reachable over the network.
pub fn ensure_bootstrap_admin(conn: &Connection) -> Option<(String, String)> {
    if user_count(conn) > 0 {
        return None;
    }
    let password = random_hex(12);
    match create_user(conn, "admin", &password, "admin") {
        Ok(_) => {
            info!("Created bootstrap admin account");
            Some(("admin".to_string(), password))
        }
        Err(e) => {
            log::error!("Could not create bootstrap admin: {}", e);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::db::open_db;

    fn mem() -> Connection {
        let path = std::env::temp_dir().join(format!(
            "stlib_auth_{}_{}.db",
            std::process::id(),
            random_hex(4)
        ));
        let conn = open_db(&path).unwrap();
        std::fs::remove_file(&path).ok();
        conn
    }

    #[test]
    fn correct_password_verifies_and_wrong_one_does_not() {
        let hash = hash_password("correct horse battery").unwrap();
        assert!(verify_password("correct horse battery", &hash));
        assert!(!verify_password("Correct horse battery", &hash));
        assert!(!verify_password("", &hash));
    }

    #[test]
    fn the_same_password_hashes_differently_each_time() {
        // Distinct salts. Equal hashes here would mean the store leaks which
        // users share a password.
        let a = hash_password("hunter2hunter2").unwrap();
        let b = hash_password("hunter2hunter2").unwrap();
        assert_ne!(a, b);
        assert!(verify_password("hunter2hunter2", &a));
        assert!(verify_password("hunter2hunter2", &b));
    }

    #[test]
    fn a_garbage_stored_hash_rejects_rather_than_panics() {
        assert!(!verify_password("anything", "not-a-phc-string"));
        assert!(!verify_password("anything", ""));
    }

    #[test]
    fn login_issues_a_token_that_verifies_and_logout_revokes_it() {
        let conn = mem();
        create_user(&conn, "dan", "printerfarm99", "admin").unwrap();

        assert!(login(&conn, "dan", "wrong-password").is_none());
        let session = login(&conn, "dan", "printerfarm99").expect("login should succeed");
        assert_eq!(session.user.username, "dan");
        assert!(session.user.is_admin());

        let who = verify_token(&conn, &session.token).expect("token should verify");
        assert_eq!(who.id, session.user.id);

        logout(&conn, &session.token);
        assert!(verify_token(&conn, &session.token).is_none());
    }

    #[test]
    fn two_logins_get_different_tokens() {
        let conn = mem();
        create_user(&conn, "dan", "printerfarm99", "admin").unwrap();
        let a = login(&conn, "dan", "printerfarm99").unwrap().token;
        let b = login(&conn, "dan", "printerfarm99").unwrap().token;
        assert_ne!(a, b, "session tokens must not be predictable or shared");
    }

    #[test]
    fn an_expired_session_stops_verifying() {
        let conn = mem();
        let user = create_user(&conn, "dan", "printerfarm99", "user").unwrap();
        conn.execute(
            "INSERT INTO sessions (token, user_id, expires_at) VALUES (?1, ?2, ?3)",
            params!["stale-token", user.id, now() - 1],
        )
        .unwrap();
        assert!(verify_token(&conn, "stale-token").is_none());
    }

    #[test]
    fn unknown_user_and_unknown_token_are_rejected() {
        let conn = mem();
        assert!(login(&conn, "nobody", "whatever123").is_none());
        assert!(verify_token(&conn, "made-up").is_none());
        // The old stub accepted this pair unconditionally.
        assert!(login(&conn, "admin", "admin").is_none());
    }

    #[test]
    fn bootstrap_admin_is_created_once_with_a_generated_password() {
        let conn = mem();
        let (user, password) = ensure_bootstrap_admin(&conn).expect("first call creates admin");
        assert_eq!(user, "admin");
        assert!(password.len() >= 16, "generated password must not be trivial");
        assert_ne!(password, "admin");
        assert!(login(&conn, "admin", &password).is_some());
        // Second call must not mint another one.
        assert!(ensure_bootstrap_admin(&conn).is_none());
    }

    #[test]
    fn short_passwords_are_refused() {
        let conn = mem();
        assert!(create_user(&conn, "dan", "short", "user").is_err());
        assert!(create_user(&conn, "  ", "longenough123", "user").is_err());
    }
}
