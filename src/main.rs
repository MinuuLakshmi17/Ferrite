use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use ferrite::{
    dht::{default_bootstrap, DhtClient},
    peer::{random_peer_id, Peer},
    piece::PieceManager,
    storage::Storage,
    torrent::TorrentMeta,
    tracker,
};
use std::{path::PathBuf, sync::Arc};
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(
    name = "ferrite",
    version,
    about = "A Rust BitTorrent client built for learning distributed systems"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand, Debug)]
enum Command {
    Inspect {
        torrent: PathBuf,
    },
    Download {
        torrent: PathBuf,
        #[arg(short, long, default_value = "downloads")]
        output: PathBuf,
        #[arg(long, default_value_t = 8)]
        peers: usize,
    },
    Dht {
        info_hash: String,
        #[arg(short, long, default_value_t = 8)]
        peers: usize,
    },
}
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    match cli.command {
        Command::Inspect { torrent } => inspect(torrent)?,
        Command::Download { torrent, output, peers } => download(torrent, output, peers).await?,
        Command::Dht { info_hash, peers } => dht(info_hash, peers).await?,
    };
    Ok(())
}
fn inspect(path: PathBuf) -> Result<()> {
    let t = TorrentMeta::from_file(&path)?;
    println!("Name       : {}", t.info.name);
    println!("Info hash  : {}", hex::encode(t.info_hash));
    println!("Size       : {} bytes", t.total_length());
    println!("Piece size : {} bytes", t.info.piece_length);
    println!("Pieces     : {}", t.piece_count());
    println!("Trackers   :");
    for x in &t.announce {
        println!("  {x}");
    }
    if !t.info.files.is_empty() {
        println!("Files      : {}", t.info.files.len());
        for f in &t.info.files {
            println!("  {:>12} {}", f.length, f.path.display());
        }
    }
    Ok(())
}
async fn download(path: PathBuf, output: PathBuf, max_peers: usize) -> Result<()> {
    let t = TorrentMeta::from_file(&path)?;
    info!(name=%t.info.name,hash=%hex::encode(t.info_hash),"loaded torrent");
    std::fs::create_dir_all(&output)?;
    let storage = Arc::new(Storage::new(&output, t.info.clone()));
    storage.preallocate()?;
    let manager = PieceManager::new(t.info.pieces.clone());
    let mut pid = random_peer_id();
    pid[4..].copy_from_slice(b"ferrite000000000");
    let response = tracker::announce(&t, pid, 6881, 0, 0, t.total_length())
        .await
        .context("tracker announce failed")?;
    println!(
        "Tracker returned {} peers (interval {}s)",
        response.peers.len(),
        response.interval
    );
    let mut handles = Vec::new();
    for p in response.peers.into_iter().take(max_peers) {
        let addr = format!("{}:{}", p.ip, p.port);
        let socket = match addr.parse() {
            Ok(x) => x,
            Err(_) => continue,
        };
        let tt = t.clone();
        let mm = manager.clone();
        let ss = storage.clone();
        handles.push(tokio::spawn(async move {
            match Peer::connect(socket, &tt, pid).await {
                Ok(peer) => peer.run(tt, mm, ss).await.unwrap_or(0),
                Err(e) => {
                    warn!(%socket,error=%e,"peer connection failed");
                    0
                }
            }
        }));
    }
    let mut total = 0;
    for h in handles {
        total += h.await?;
    }
    println!("Downloaded {} bytes; complete={}", total, manager.is_complete().await);
    if !manager.is_complete().await {
        println!("Not complete: retry with more peers or another tracker.");
    }
    Ok(())
}
async fn dht(s: String, max: usize) -> Result<()> {
    let clean = s.strip_prefix("0x").unwrap_or(&s);
    let bytes = hex::decode(clean).context("info hash must be 40 hex characters")?;
    if bytes.len() != 20 {
        anyhow::bail!("info hash must be 20 bytes")
    };
    let mut hash = [0; 20];
    hash.copy_from_slice(&bytes);
    let client = DhtClient::bind(random_peer_id()).await?;
    let peers = client.get_peers(hash, &default_bootstrap()).await?;
    println!("DHT discovered {} peers", peers.len());
    for p in peers.into_iter().take(max) {
        println!("{}", p.addr);
    }
    Ok(())
}
