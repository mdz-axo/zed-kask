//! REPL /matrix and /msg handlers — Matrix chat interface in the TUI.
//!
//! /matrix [ROOM]  — list rooms, or show recent messages from a room
//! /msg ROOM TEXT  — send a message to a Matrix room

use hkask_communication::matrix::{MatrixTransport, RoomId};

/// Handle /matrix — list rooms or show messages from a specific room.
pub fn handle_matrix(arg: &str, rt: &tokio::runtime::Handle) {
    let homeserver_url =
        std::env::var("HKASK_MATRIX_URL").unwrap_or_else(|_| "http://localhost:8008".to_string());

    rt.block_on(async {
        let mut transport = MatrixTransport::new(&homeserver_url);

        // Try to log in if credentials are available
        if let (Ok(username), Ok(password)) = (
            std::env::var("HKASK_MATRIX_AGENT_USERNAME"),
            std::env::var("HKASK_MATRIX_AGENT_PASSWORD"),
        ) {
            if let Err(e) = transport.login(&username, &password).await {
                println!("  \x1b[31m✗\x1b[0m Matrix login failed: {}", e);
                println!("  Set HKASK_MATRIX_AGENT_USERNAME and HKASK_MATRIX_AGENT_PASSWORD");
                println!();
                return;
            }
        } else {
            println!("  \x1b[33m⚠\x1b[0m Not logged in — set HKASK_MATRIX_AGENT_USERNAME/PASSWORD");
            println!("  Showing homeserver health only.");
            println!();
            match transport.health_check().await {
                Ok(true) => println!("  Conduit: \x1b[32mreachable\x1b[0m at {}", homeserver_url),
                Ok(false) => println!("  Conduit: \x1b[31munreachable\x1b[0m"),
                Err(e) => println!("  Conduit: \x1b[31merror\x1b[0m — {}", e),
            }
            println!();
            return;
        }

        if arg.is_empty() {
            // List rooms
            match transport.list_rooms().await {
                Ok(rooms) => {
                    if rooms.is_empty() {
                        println!("  No rooms joined.");
                        println!(
                            "  Create one: \x1b[36m/create_thread\x1b[0m or use a Matrix client."
                        );
                    } else {
                        println!("  \x1b[1mMatrix Rooms\x1b[0m ({})", rooms.len());
                        for room in &rooms {
                            let member_count = room.participants.len();
                            println!(
                                "    \x1b[36m{}\x1b[0m  {}  ({} members)",
                                room.room_id.as_str(),
                                room.title,
                                member_count,
                            );
                        }
                        println!();
                        println!("  View messages: \x1b[36m/matrix <room_id>\x1b[0m");
                    }
                }
                Err(e) => println!("  \x1b[31m✗\x1b[0m Failed to list rooms: {}", e),
            }
        } else {
            // Show recent messages from a specific room
            let room_id = RoomId::new(arg);
            match transport.get_messages(&room_id, 20).await {
                Ok(messages) => {
                    if messages.is_empty() {
                        println!("  No messages in room {}", arg);
                    } else {
                        println!("  \x1b[1mRecent messages in {}\x1b[0m", arg);
                        for msg in &messages {
                            let time = chrono::DateTime::from_timestamp(msg.timestamp, 0)
                                .map(|dt| dt.format("%H:%M").to_string())
                                .unwrap_or_else(|| "??:??".to_string());
                            println!(
                                "    \x1b[2m[{}]\x1b[0m \x1b[1m{}\x1b[0m: {}",
                                time,
                                msg.sender.as_str(),
                                msg.body,
                            );
                        }
                    }
                }
                Err(e) => println!("  \x1b[31m✗\x1b[0m Failed to get messages: {}", e),
            }
        }
        println!();
    });
}

/// Handle /msg — send a message to a Matrix room.
pub fn handle_msg(room_arg: &str, text_arg: &str, rt: &tokio::runtime::Handle) {
    if room_arg.is_empty() || text_arg.is_empty() {
        println!("  Usage: \x1b[36m/msg <room_id> <message>\x1b[0m");
        println!();
        return;
    }

    let homeserver_url =
        std::env::var("HKASK_MATRIX_URL").unwrap_or_else(|_| "http://localhost:8008".to_string());

    rt.block_on(async {
        let mut transport = MatrixTransport::new(&homeserver_url);

        let (username, password) = match (
            std::env::var("HKASK_MATRIX_AGENT_USERNAME"),
            std::env::var("HKASK_MATRIX_AGENT_PASSWORD"),
        ) {
            (Ok(u), Ok(p)) => (u, p),
            _ => {
                println!("  \x1b[31m✗\x1b[0m Matrix credentials not set.");
                println!("  Set HKASK_MATRIX_AGENT_USERNAME and HKASK_MATRIX_AGENT_PASSWORD");
                println!();
                return;
            }
        };

        if let Err(e) = transport.login(&username, &password).await {
            println!("  \x1b[31m✗\x1b[0m Login failed: {}", e);
            println!();
            return;
        }

        let room_id = RoomId::new(room_arg);
        match transport.send_message(&room_id, text_arg, None).await {
            Ok(()) => println!("  \x1b[32m✓\x1b[0m Message sent to {}", room_arg),
            Err(e) => println!("  \x1b[31m✗\x1b[0m Send failed: {}", e),
        }
        println!();
    });
}
