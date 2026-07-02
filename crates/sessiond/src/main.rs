//! bpa-sessiond — Builder Pro AI session daemon.
//! S0 skeleton: arg parse + startup log. PTY/socket/persistence land in Task 4–Task 13.

fn main() {
    // LaunchAgent invokes: bpa-sessiond --socket <RESOLVED_SOCKET_PATH> (spec §8.3).
    let mut socket_path: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--socket" => socket_path = args.next(),
            "--version" => {
                println!("bpa-sessiond {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            other => {
                eprintln!("bpa-sessiond: unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }

    eprintln!(
        "bpa-sessiond {} starting; proto={} socket={:?}",
        env!("CARGO_PKG_VERSION"),
        bpa_sessiond::protocol::PROTO_VERSION,
        socket_path,
    );
    // S0 skeleton exits immediately (clean). The serve loop is added in Task 12/Task 13.
}
