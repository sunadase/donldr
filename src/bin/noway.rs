use std::{borrow::Borrow, path::PathBuf};

use clap::Parser;
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
    chunks: u8,
}

#[tokio::main]
async fn main() -> DResult<()> {
    println!("Hello, world!");

    #[cfg(debug_assertions)]
    let tracing_level = tracing::Level::DEBUG;
    #[cfg(not(debug_assertions))]
    let tracing_level = tracing::Level::ERROR;
    let subcriber = tracing_subscriber::fmt()
        .compact()
        .with_line_number(true)
        .with_thread_ids(true)
        .with_max_level(tracing_level)
        .finish();
    subscriber::set_global_default(subcriber)?;

    let c = Cli::parse();
    debug!("parsed cli:\n{:#?}", c);
    let url = reqwest::Url::parse(&c.url).expect("Failed parsing url");
    debug!("parsed url:\n{:#?}", &url);

    let client = reqwest::Client::new();
    let hdr = client.head(url.as_str()).send().await?;
    debug!("headers at target url:\n{:#?}", hdr);

    let size = hdr
        .headers()
        .get("content-length")
        .expect("Failed to get content length")
        .to_str()
        .unwrap()
        .parse::<u64>()
        .expect("Failed parsing content-length");
    debug!("size: {}", size);
    let chunk_size = size / c.chunks as u64;
    debug!("chunk size: {}", chunk_size);

    let accept_ranges = hdr
        .headers()
        .get("accept-ranges")
        .expect("Can't find accept ranges");
    debug!("accept ranges: {:?}", accept_ranges);

    let mut ranges = (0..size)
        .step_by(chunk_size as usize)
        .map(|from| (from, from + chunk_size - 1))
        .collect::<Vec<_>>();
    ranges.last_mut().expect("Failed getting last range").1 = size;

    debug!("ranges:\n{:?}", ranges);

    let file_path = get_file_path(&c);

    debug!("Parsed target file path as:\n {:?}", file_path);

    download_files(c.url.as_str(), &file_path).await?;

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

fn get_file_path(c:&Cli) -> PathBuf{
    let mut p = PathBuf::from(c.path.to_owned());
    let filename = match &c.url.rsplit_once("/") {
        None => "download.bin",
        Some((s1, s2)) => s2,
    };
    debug!("filename: {}", filename);
    if p.is_dir() {
        p.push(filename);
        if p.is_file() {
            warn!("File already exists? will overwrite??!");
            p
        } else {
            p
        }
    } else {
        debug!("p: {:?}", p);
        if p.is_file() {
            warn!("File already exists? will overwrite??!");
            p
        } else {
            p
        }
    }
}


#[derive(Debug)]
enum Errors {
    Tracing(tracing::subscriber::SetGlobalDefaultError),
    Io(std::io::Error),
    Reqwest(reqwest::Error),
}

impl From<SetGlobalDefaultError> for Errors {
    fn from(value: SetGlobalDefaultError) -> Self {
        Errors::Tracing(value)
    }
}
impl From<std::io::Error> for Errors {
    fn from(value: std::io::Error) -> Self {
        Errors::Io(value)
    }
}
impl From<reqwest::Error> for Errors {
    fn from(value: reqwest::Error) -> Self {
        Errors::Reqwest(value)
    }
}

type DResult<T> = Result<T, Errors>;
