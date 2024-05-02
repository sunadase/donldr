use std::{borrow::Borrow, path::PathBuf};

use clap::Parser;
use donldr::{download::{determine_file_path, Download}, DResult, Errors};
use futures::StreamExt;
use reqwest::Client;
use tokio::{fs::File, io::AsyncWriteExt, time::Instant};
use tokio_util::bytes::BufMut;
use tracing::{
    debug,
    subscriber::{self, SetGlobalDefaultError},
    warn,
};

// mod main_tokio;

struct DownloadTask {
    url: String,
    path: String,
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    ///URL to download file from
    #[arg(short, long)]
    url: String, //can change to vec later to support multiple files
    ///Target path to save the file
    #[arg(short, long, default_value = "./")]
    path: String,
    ///Chunks to divide the file into concurrent downloads
    #[arg(short, long, default_value_t = 8)]
    chunks: usize,
}

#[tokio::main]
async fn main() -> DResult<()> {
    donldr::set_tracing()?;

    let c = Cli::parse();
    debug!("parsed cli:\n{:#?}", c);
    let download = Download::new(c.url, c.path, c.chunks).await?;

    let file_path = determine_file_path(&download.path, &download.url);

    debug!("Parsed target file path as:\n {:?}", file_path);

    download_files(&download.url, &file_path).await?;

    Ok(())
}

async fn download_files(url: &str, path: &PathBuf) -> Result<(), Errors> {
    let start_time = Instant::now();
    let mut file = File::create(path).await?;
    println!("Downloading {}...", url);

    let mut stream = reqwest::get(url).await?.bytes_stream();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        file.write_all(&chunk).await?;
    }

    file.flush().await?;

    println!("Downloaded in {:?}", start_time.elapsed());
    Ok(())
}
