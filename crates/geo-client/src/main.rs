use clap::Parser;

#[derive(Parser)]
#[command(name = "geo-client", about = "client camionista")]
struct Args {
    #[arg(long, env = "GEO_SERVER", default_value = "127.0.0.1:7878")]
    server: String,

    #[arg(long, env = "GEO_USER")]
    username: String,

    #[arg(long, env = "GEO_PASS")]
    password: String,

    #[arg(long, env = "GEO_TRACK")]
    track: Option<std::path::PathBuf>,

    #[arg(long, default_value_t = 1)]
    time_scale: u32,

    #[arg(long, default_value_t = false)]
    headless: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    todo!()
}