//! UserPod registration and authentication — delegates to AgentService.

use std::sync::{Arc, Mutex};

use crate::cli::UserPodAction;
use crate::error::CliError;
use hkask_identity::{RegistrationRequest, UserPod, UserSession};
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use hkask_storage::user_store::UserStore;
use hkask_types::UserID;
use zeroize::Zeroizing;

type Store = Arc<Mutex<UserStore>>;

fn build_store() -> Store {
    crate::commands::helpers::build_agent_service()
        .storage()
        .users
        .clone()
        .clone()
}

/// Unwrap an I/O result or print the error and exit.
/// Used for interactive stdin/stdout operations where failure is terminal.
fn io_or_die<T>(result: std::io::Result<T>, context: &str) -> T {
    result.unwrap_or_else(|e| {
        eprintln!("I/O error ({context}): {e}");
        std::process::exit(1);
    })
}

fn validate_passphrase(passphrase: &str) -> Result<(), ServiceError> {
    // Security: 8+ alphanumeric is sufficient because Argon2id memory-hard
    // hashing (in UserStore::hash_passphrase) makes brute-force infeasible
    // even for lowercase-only passphrases. 36^8 = 2.8T combinations at ~100ms
    // per Argon2id attempt = ~9000 years. Mixed-case was previously required
    // but removed because it prevented valid passphrases like "allostery".
    if passphrase.len() < 8 || !passphrase.chars().all(|c| c.is_alphanumeric()) {
        return Err(ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::User,
            source: None,
            message: "Passphrase does not meet requirements: 8+ alphanumeric chars".into(),
        });
    }
    Ok(())
}

fn validate_registration(request: &RegistrationRequest) -> Result<(), ServiceError> {
    if request.userpod_name.is_empty() || request.userpod_name.len() > 64 {
        return Err(ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::User,
            source: None,
            message: "Invalid userpod name".into(),
        });
    }
    if request.first_name.is_empty() || request.last_name.is_empty() {
        return Err(ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::User,
            source: None,
            message: "Required name field is empty".into(),
        });
    }
    if request.email.is_empty() || !request.email.contains('@') {
        return Err(ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::User,
            source: None,
            message: "Invalid contact information format".into(),
        });
    }
    if let Some(phone) = &request.phone
        && !phone.starts_with('+')
    {
        return Err(ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::User,
            source: None,
            message: "Invalid contact information format".into(),
        });
    }
    validate_passphrase(&request.passphrase)?;
    Ok(())
}

/// pre:  store is a valid UserStore; userpod_name, first_name, last_name, email are non-empty; passphrase meets validation (8+ alphanumeric, mixed case)
/// post: registers a new userpod identity in the store; returns UserPod on success or ServiceError on validation/store failure
pub fn register_userpod_with_passphrase(
    store: &Store,
    userpod_name: &str,
    first_name: &str,
    last_name: &str,
    email: &str,
    phone: Option<&str>,
    passphrase: Zeroizing<String>,
) -> Result<UserPod, ServiceError> {
    validate_passphrase(&passphrase)?;
    let request = RegistrationRequest {
        userpod_name: userpod_name.to_string(),
        first_name: first_name.to_string(),
        last_name: last_name.to_string(),
        email: email.to_string(),
        phone: phone.map(|s| s.to_string()),
        passphrase: (*passphrase).clone(),
        capabilities: hkask_identity::UserPod::DEFAULT_CAPABILITIES
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };
    validate_registration(&request)?;
    store
        .lock()
        .expect("CLI operation")
        .register_userpod(&request)
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: e.to_string(),
        })
}

/// pre:  store is a valid UserStore; userpod_name is non-empty; passphrase is the correct credential
/// post: returns a UserSession on successful authentication or ServiceError::LoginFailed on invalid credentials
pub fn login_with_passphrase(
    store: &Store,
    userpod_name: &str,
    passphrase: Zeroizing<String>,
) -> Result<UserSession, ServiceError> {
    store
        .lock()
        .expect("CLI operation")
        .login(userpod_name, &passphrase)
        .map_err(|_| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::User,
            source: None,
            message: "Invalid credentials".into(),
        })
}

/// pre:  store is a valid UserStore; userpod_name is non-empty
/// post: returns the UserPod if found, or ServiceError::UserNotFound if the userpod does not exist
pub fn get_userpod(store: &Store, userpod_name: &str) -> Result<UserPod, ServiceError> {
    store
        .lock()
        .expect("CLI operation")
        .get_userpod(userpod_name)
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: e.to_string(),
        })?
        .ok_or_else(|| ServiceError::Domain {
            kind: ErrorKind::NotFound,
            domain: DomainKind::User,
            source: None,
            message: format!("UserPod '{}'", userpod_name),
        })
}

/// pre:  store is a valid UserStore; user_id is a valid UserID
/// post: returns all userpod identities belonging to the given user; empty vec if none
pub fn get_userpods(store: &Store, user_id: &UserID) -> Result<Vec<UserPod>, ServiceError> {
    let userpod = store
        .lock()
        .expect("CLI operation")
        .get_userpod_by_user(user_id)
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: e.to_string(),
        })?;
    Ok(userpod.into_iter().collect())
}

/// pre:  store is a valid UserStore; userpod_name is non-empty
/// post: returns all active sessions for the userpod; empty vec if none
pub fn get_sessions(store: &Store, userpod_name: &str) -> Result<Vec<UserSession>, ServiceError> {
    store
        .lock()
        .expect("CLI operation")
        .list_sessions(userpod_name)
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: e.to_string(),
        })
}

/// pre:  store is a valid UserStore; session_id is a non-empty session identifier
/// post: revokes the session (logs out) and returns the revoked UserSession; ServiceError if session not found
pub fn revoke_session(store: &Store, session_id: &str) -> Result<UserSession, ServiceError> {
    let session = store
        .lock()
        .expect("CLI operation")
        .get_session(session_id)
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: e.to_string(),
        })?
        .ok_or_else(|| ServiceError::Domain {
            kind: ErrorKind::NotFound,
            domain: DomainKind::User,
            source: None,
            message: format!("Session '{}'", session_id),
        })?;
    store
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .logout(session_id)
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: e.to_string(),
        })?;
    Ok(session)
}

/// pre:  stdin is available for interactive input
/// post: prompts user for userpod details and passphrase; registers on success, prints ✓; exits on validation failure
/// Register a new userpod identity (interactive)
pub fn register_userpod() {
    use std::io::{self, Write};
    let mut name = String::new();
    let mut first = String::new();
    let mut last = String::new();
    let mut email = String::new();
    let mut phone = String::new();

    print!("UserPod name: ");
    io_or_die(io::stdout().flush(), "flush stdout");
    io_or_die(io::stdin().read_line(&mut name), "read name");
    print!("First name: ");
    io_or_die(io::stdout().flush(), "flush stdout");
    io_or_die(io::stdin().read_line(&mut first), "read first name");
    print!("Last name: ");
    io_or_die(io::stdout().flush(), "flush stdout");
    io_or_die(io::stdin().read_line(&mut last), "read last name");
    print!("Email: ");
    io_or_die(io::stdout().flush(), "flush stdout");
    io_or_die(io::stdin().read_line(&mut email), "read email");
    print!("Phone (optional): ");
    io_or_die(io::stdout().flush(), "flush stdout");
    io_or_die(io::stdin().read_line(&mut phone), "read phone");

    loop {
        print!("Enter passphrase: ");
        io_or_die(io::stdout().flush(), "flush stdout");
        let mut passphrase = String::new();
        io_or_die(io::stdin().read_line(&mut passphrase), "read passphrase");
        let passphrase = passphrase.trim().to_string();
        if let Err(e) = validate_passphrase(&passphrase) {
            eprintln!("  ✗ {}", e);
            continue;
        }
        let store = build_store();
        match register_userpod_with_passphrase(
            &store,
            name.trim(),
            first.trim(),
            last.trim(),
            email.trim(),
            if phone.trim().is_empty() {
                None
            } else {
                Some(phone.trim())
            },
            Zeroizing::new(passphrase),
        ) {
            Ok(identity) => {
                println!("  ✓ UserPod registered: {}", identity.userpod_name);
                return;
            }
            Err(e) => {
                eprintln!("  ✗ {}", e);
                break;
            }
        }
    }
}

/// pre:  stdin is available; userpod must exist in store
/// post: prompts for name and passphrase; prints session info on success or error message on failure
pub fn login_userpod() {
    use std::io::{self, Write};
    let mut name = String::new();
    print!("UserPod name: ");
    io_or_die(io::stdout().flush(), "flush stdout");
    io_or_die(io::stdin().read_line(&mut name), "read name");
    let store = build_store();
    if let Ok(Some(identity)) = store
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_userpod(name.trim())
    {
        print!("Enter passphrase: ");
        io_or_die(io::stdout().flush(), "flush stdout");
        let mut passphrase = String::new();
        io_or_die(io::stdin().read_line(&mut passphrase), "read passphrase");
        match store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .login(name.trim(), passphrase.trim())
        {
            Ok(session) => {
                println!("  ✓ Logged in as {}", identity.userpod_name);
                println!("  Session: {}", session.session_id);
            }
            Err(_) => eprintln!("  ✗ Login failed"),
        }
    } else {
        eprintln!("  ✗ UserPod not found: {}", name.trim());
    }
}

/// pre:  store is a valid UserStore; userpod_name is non-empty and exists
/// post: prints userpod details (name, user_id, created_at) to stdout; ServiceError if not found
pub fn show_userpod(store: &Store, userpod_name: &str) -> Result<(), ServiceError> {
    let identity = store
        .lock()
        .expect("CLI operation")
        .get_userpod(userpod_name)
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: e.to_string(),
        })?
        .ok_or_else(|| ServiceError::Domain {
            kind: ErrorKind::NotFound,
            domain: DomainKind::User,
            source: None,
            message: format!("UserPod '{}'", userpod_name),
        })?;
    println!("UserPod: {}", identity.userpod_name);
    println!("  User ID: {}", identity.user_id);
    println!("  Created: {}", identity.created_at);
    Ok(())
}

/// pre:  store is a valid UserStore
/// post: prints all userpods with name, user_id, and created_at; prints "No userpods registered." if empty
pub fn list_userpods(store: &Store) -> Result<(), ServiceError> {
    let userpods = store
        .lock()
        .expect("CLI operation")
        .list_userpods()
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: e.to_string(),
        })?;
    if userpods.is_empty() {
        println!("No userpods registered.");
        return Ok(());
    }
    println!("UserPods ({}):", userpods.len());
    for r in userpods {
        println!("  {}", r.userpod_name);
        println!("    User ID: {}", r.user_id);
        println!("    Created: {}", r.created_at);
    }
    Ok(())
}

/// pre:  store is a valid UserStore; session_id is a non-empty active session identifier
/// post: revokes the session and prints confirmation; ServiceError if session not found
pub fn logout(store: &Store, session_id: &str) -> Result<(), ServiceError> {
    let session = store
        .lock()
        .expect("CLI operation")
        .get_session(session_id)
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: e.to_string(),
        })?
        .ok_or_else(|| ServiceError::Domain {
            kind: ErrorKind::NotFound,
            domain: DomainKind::User,
            source: None,
            message: format!("Session '{}'", session_id),
        })?;
    store
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .logout(session_id)
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: e.to_string(),
        })?;
    println!("Session revoked: {}", session.session_id);
    Ok(())
}

/// pre:  store is a valid UserStore; userpod_name is non-empty
/// post: prints all active sessions with session_id and last_active timestamp; prints "No active sessions." if none
pub fn list_sessions(store: &Store, userpod_name: &str) -> Result<(), ServiceError> {
    let sessions = store
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .list_sessions(userpod_name)
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: e.to_string(),
        })?;
    if sessions.is_empty() {
        println!("No active sessions.");
        return Ok(());
    }
    println!("Active sessions for {}: {}", userpod_name, sessions.len());
    for s in sessions {
        let last_active = chrono::DateTime::from_timestamp(s.last_active, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default();
        println!("  Session: {}", s.session_id);
        println!("     Last active: {}", last_active);
    }
    Ok(())
}

pub(crate) struct InviteTarget {
    name: String,
    email: String,
}

/// Parse `Name <email>` format. Returns Err if format is invalid.
fn parse_invitee(input: &str) -> Result<InviteTarget, CliError> {
    let input = input.trim();
    let (name, email) = if let Some(lt) = input.rfind('<') {
        let rt = input
            .rfind('>')
            .ok_or_else(|| CliError::InvalidInput("missing closing '>' in invitee".into()))?;
        let name = input[..lt].trim().to_string();
        let email = input[lt + 1..rt].trim().to_string();
        if name.is_empty() {
            return Err(CliError::InvalidInput("name is empty".into()));
        }
        if email.is_empty() || !email.contains('@') {
            return Err(CliError::InvalidInput(format!("invalid email: {email}")));
        }
        (name, email)
    } else if input.contains('@') {
        // Just an email, no name
        ("there".to_string(), input.to_string())
    } else {
        return Err(CliError::InvalidInput(
            "expected 'Name <email>' or just an email address".into(),
        ));
    };
    Ok(InviteTarget { name, email })
}

/// Create an invite code for a new member.
///
/// If target is None, just prints the code (no email metadata).
/// If target is Some, prints code + sends email if --send.
pub(crate) fn create_invite(
    store: &Store,
    admin_userpod: &str,
    target: Option<&InviteTarget>,
    send_email_flag: bool,
    rt: &tokio::runtime::Runtime,
) -> Result<(), ServiceError> {
    let userpod = store
        .lock()
        .expect("CLI operation")
        .get_userpod(admin_userpod)
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: e.to_string(),
        })?
        .ok_or_else(|| ServiceError::Domain {
            kind: ErrorKind::NotFound,
            domain: DomainKind::User,
            source: None,
            message: format!("UserPod '{}'", admin_userpod),
        })?;
    let invite = store
        .lock()
        .expect("CLI operation")
        .create_invite(&userpod.user_id)
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: e.to_string(),
        })?;
    println!("Invite code: {}", invite.code);
    println!(
        "Expires: {}",
        chrono::DateTime::from_timestamp(invite.expires_at, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_default()
    );

    // Print acceptance link
    let domain = std::env::var("HKASK_DOMAIN").unwrap_or_else(|_| "localhost".to_string());
    let scheme = if domain == "localhost" {
        "http"
    } else {
        "https"
    };
    println!(
        "Acceptance link: {scheme}://{domain}/api/v1/auth/accept-invite?code={}",
        invite.code
    );

    // Send email if requested
    if send_email_flag {
        let t = target.ok_or_else(|| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::User,
            source: None,
            message: "--send requires an invitee (--invitee or --invitees)".into(),
        })?;
        println!("Sending invite email to {}...", t.email);
        rt.block_on(async {
            hkask_api::email::send_invite_email(&t.email, &t.name, &invite.code)
                .await
                .map_err(|e| ServiceError::Domain {
                    kind: ErrorKind::BadRequest,
                    domain: DomainKind::Infrastructure,
                    source: Some(Box::new(e)),
                    message: "Email send failed".into(),
                })
        })?;
        println!("Invite email sent.");
    } else {
        println!("Share this code with the user, or use --send to email it.");
    }
    Ok(())
}

/// Revoke a pending invite code.
///
/// expect: "As an admin I can revoke an invite"
/// pre:  admin_userpod is valid; code is a pending invite owned by admin
/// post: prints confirmation; returns error if invite not found or already accepted
pub fn revoke_invite(store: &Store, admin_userpod: &str, code: &str) -> Result<(), ServiceError> {
    let userpod = store
        .lock()
        .expect("CLI operation")
        .get_userpod(admin_userpod)
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: e.to_string(),
        })?
        .ok_or_else(|| ServiceError::Domain {
            kind: ErrorKind::NotFound,
            domain: DomainKind::User,
            source: None,
            message: format!("UserPod '{}'", admin_userpod),
        })?;
    let invite = store
        .lock()
        .expect("CLI operation")
        .revoke_invite(code, &userpod.user_id)
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: e.to_string(),
        })?;
    println!("Invite {} revoked.", invite.code);
    Ok(())
}

/// pre:  action is a valid UserPodAction variant
/// post: dispatches to the appropriate handler (register, login, show, list, sessions, logout, passphrase); prints results or errors
pub fn run_userpod(rt: &tokio::runtime::Runtime, action: crate::cli::UserPodAction) {
    match action {
        UserPodAction::Register { .. } => register_userpod(),
        UserPodAction::Login { .. } => login_userpod(),
        UserPodAction::Show { userpod_name } => {
            let store = build_store();
            super::helpers::or_exit(show_userpod(&store, &userpod_name), "Show failed");
        }
        UserPodAction::List { .. } => {
            let store = build_store();
            super::helpers::or_exit(list_userpods(&store), "List failed");
        }
        UserPodAction::Sessions { userpod_name } => {
            let store = build_store();
            super::helpers::or_exit(list_sessions(&store, &userpod_name), "Sessions failed");
        }
        UserPodAction::Logout { session_id } => {
            let store = build_store();
            super::helpers::or_exit(logout(&store, &session_id), "Logout failed");
        }
        UserPodAction::Passphrase { userpod_name } => {
            change_passphrase(&userpod_name);
        }
        UserPodAction::Rename { from, to } => {
            userpod_rename(rt, &from, &to);
        }
        UserPodAction::Delete { name } => {
            userpod_delete(rt, &name);
        }
        UserPodAction::Invite {
            by,
            invitee,
            invitees,
            send,
        } => {
            let store = build_store();
            let mut targets: Vec<InviteTarget> = Vec::new();
            if let Some(ref inv) = invitee {
                match parse_invitee(inv) {
                    Ok(t) => targets.push(t),
                    Err(e) => {
                        eprintln!("Invalid invitee format '{}': {}", inv, e);
                        std::process::exit(1);
                    }
                }
            }
            for inv in &invitees {
                match parse_invitee(inv) {
                    Ok(t) => targets.push(t),
                    Err(e) => {
                        eprintln!("Invalid invitee format '{}': {}", inv, e);
                        std::process::exit(1);
                    }
                }
            }
            if targets.is_empty() {
                super::helpers::or_exit(
                    create_invite(&store, &by, None, send, rt),
                    "Invite failed",
                );
            } else {
                let mut ok = 0u32;
                let mut failed = 0u32;
                println!("Creating {} invites...", targets.len());
                for target in &targets {
                    print!("  {}: ", target.name);
                    match create_invite(&store, &by, Some(target), send, rt) {
                        Ok(()) => {
                            ok += 1;
                            println!("✓");
                        }
                        Err(e) => {
                            failed += 1;
                            eprintln!("✗ ({})", e);
                        }
                    }
                }
                println!("Done: {} sent, {} failed.", ok, failed);
                if failed > 0 {
                    std::process::exit(1);
                }
            }
        }
        UserPodAction::RevokeInvite { code, by } => {
            let store = build_store();
            super::helpers::or_exit(revoke_invite(&store, &by, &code), "Revoke failed");
        }
    }
}

/// Rename a userpod via the API.
fn userpod_rename(rt: &tokio::runtime::Runtime, from: &str, to: &str) {
    rt.block_on(async {
        let client = reqwest::Client::new();
        let base_url =
            std::env::var("HKASK_BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());
        let resp = client
            .post(format!("{base_url}/api/v1/userpods/rename"))
            .json(&serde_json::json!({"from": from, "to": to}))
            .send()
            .await;
        match resp {
            Ok(r) if r.status().is_success() => {
                println!("UserPod renamed: {from} -> {to}");
            }
            Ok(r) => {
                let body = r.text().await.unwrap_or_default();
                eprintln!("Rename failed: {body}");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("Request failed: {e}");
                std::process::exit(1);
            }
        }
    });
}

/// Delete a userpod via the API.
fn userpod_delete(rt: &tokio::runtime::Runtime, name: &str) {
    rt.block_on(async {
        let client = reqwest::Client::new();
        let base_url =
            std::env::var("HKASK_BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());
        let resp = client
            .delete(format!("{base_url}/api/v1/userpods/{name}"))
            .send()
            .await;
        match resp {
            Ok(r) if r.status().is_success() => {
                println!("UserPod deleted: {name}");
            }
            Ok(r) => {
                let body = r.text().await.unwrap_or_default();
                eprintln!("Delete failed: {body}");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("Request failed: {e}");
                std::process::exit(1);
            }
        }
    });
}

/// pre:  userpod_name exists in store; stdin is available for interactive input
/// post: prompts for old and new passphrase; validates match; updates passphrase and invalidates existing sessions on success
/// Interactive passphrase change for a userpod.
pub fn change_passphrase(userpod_name: &str) {
    use std::io::{self, Write};
    let store = build_store();

    // Verify identity exists
    if store
        .lock()
        .expect("CLI operation")
        .get_userpod(userpod_name)
        .unwrap_or(None)
        .is_none()
    {
        eprintln!("  ✗ UserPod not found: {}", userpod_name);
        return;
    }

    print!("Old passphrase: ");
    io_or_die(io::stdout().flush(), "flush stdout");
    let mut old_passphrase = String::new();
    io_or_die(
        io::stdin().read_line(&mut old_passphrase),
        "read old passphrase",
    );

    print!("New passphrase: ");
    io_or_die(io::stdout().flush(), "flush stdout");
    let mut new_passphrase = String::new();
    io_or_die(
        io::stdin().read_line(&mut new_passphrase),
        "read new passphrase",
    );

    print!("Confirm new passphrase: ");
    io_or_die(io::stdout().flush(), "flush stdout");
    let mut confirm = String::new();
    io_or_die(io::stdin().read_line(&mut confirm), "read confirm");

    if new_passphrase.trim() != confirm.trim() {
        eprintln!("  ✗ Passphrases do not match");
        return;
    }

    match store
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .change_passphrase(userpod_name, old_passphrase.trim(), new_passphrase.trim())
    {
        Ok(()) => {
            println!("  ✓ Passphrase changed for {}", userpod_name);
            println!("  All existing sessions invalidated — login again.");
        }
        Err(e) => eprintln!("  ✗ {}", e),
    }
}
