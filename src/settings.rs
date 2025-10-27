use clap::Parser;

#[derive(Parser)]
#[command(
    name = "United Cinemas - WebRTC SFU Server",
    version = "0.1.0",
    author = "Kaucrow & Beckarby",
    about = "United Cinemas - WebRTC SFU Server\n\nA simple WebRTC SFU implementation in Rust built for broadcasting video streams.",
    long_about = None
)]

struct Args {
    /// Server port
    #[arg(short, long, default_value_t = 8080)]
    pub port: u16,
    /// Output debug logs
    #[arg(short, long, default_value_t = false)]
    pub debug: bool
}

pub struct Settings {
    pub port: u16,
    pub debug: bool
}

impl Settings {
    pub fn new() -> Self {
        let args = Args::parse();
        Self {
            port: args.port,
            debug: args.debug
        }
    }
}