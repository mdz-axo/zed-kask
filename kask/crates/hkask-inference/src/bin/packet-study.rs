//! Offline packet-study preparation, replay and journal audit. No live transport.

fn main() -> std::process::ExitCode {
    match hkask_inference::packet_study::run_cli(std::env::args().skip(1)) {
        Ok(report) => match serde_json::to_string_pretty(&report) {
            Ok(text) => {
                println!("{text}");
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("packet-study report serialization failed: {error}");
                std::process::ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("packet-study: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
