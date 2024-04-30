use std::{ops::DerefMut, path::PathBuf};

use clap::Parser;
use tokio::{fs::File, io::{self, AsyncWriteExt}, net::TcpStream};
use tracing::{
    debug,
    subscriber::{self, SetGlobalDefaultError},
    warn,
};

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
    
    let stream = TcpStream::connect(c.url).await?;
    let (reader, writer) = stream.split();

    let head_request = http::Request::builder().method("HEAD").body("").expect("Error building the HEAD requeyst");
    writer.write_all(head_request.body().as_bytes()).await?;

    reader.

    reader.

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

    let accept_ranges = hdr.headers().get("accept-ranges").expect("Can't find accept ranges");
    debug!("accept ranges: {:?}", accept_ranges);

    let mut ranges = (0..size)
        .step_by(chunk_size as usize)
        .map(|from| (from, from + chunk_size - 1))
        .collect::<Vec<_>>();
    ranges.last_mut().expect("Failed getting last range").1 = size;

    debug!("ranges:\n{:?}", ranges);

    let file_path = {
        let mut p = PathBuf::from(c.path);
        if p.is_dir() {
            p.push(
            hdr.url()
                .path_segments()
                .expect("Failed getting path segments from url")
                // .inspect(|x| debug!("url segments: {:?}", x))
                .last()
                .inspect(|x| debug!("url last segment: {:?}", x))
                .expect("Failed getting file name from url")
                .to_owned()
            );
            p
        } else {
            if p.is_file() {
                warn!("File already exists? will overwrite??!");
                p
            } else {
                p
            }
        }
    };

    debug!("Parsed target file path as:\n {:?}", file_path);

    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .open(file_path)
        .await
        .unwrap();

    file.set_len(size).await?;


    let test_part = client.get(c.url)
    .header("Range", format!("bytes={}-{}",ranges[0].0, ranges[0].1))
    .send().await?.;

    debug!("test_part:\n{:?}", test_part);

    let mut mmap = unsafe { memmap2::MmapMut::map_mut(&file).expect("getting a mmap for file failed") };
    mmap.chunks_mut(chunk_size as usize).for_each(|mem_chunk| {
        tokio::spawn(async move {
            tokio::io::copy(reader, mem_chunk)
        });
    });


    Ok(())
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
