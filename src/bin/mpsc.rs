use std::{borrow::Borrow, collections::HashSet, fmt::write, path::PathBuf};

use clap::Parser;
use colored::Colorize;
use reqwest::Client;
use tokio::{
    fs::File,
    io::{AsyncSeekExt, AsyncWriteExt},
    sync::mpsc::{Receiver, Sender},
    task::JoinHandle,
    time::Instant,
};
use tokio_util::bytes::{BufMut, Bytes};
use tracing::{
    debug, error,
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
    #[cfg(debug_assertions)]
    let tracing_level = tracing::Level::DEBUG;
    #[cfg(not(debug_assertions))]
    let tracing_level = tracing::Level::WARN;
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
    //? if chunks>size?
    debug!("chunk size: {}", chunk_size);

    if let Some(accept_ranges) = hdr
    .headers()
    .get("accept-ranges"){
        debug!("accept ranges: {:?}", accept_ranges);
        assert_ne!(accept_ranges, "none", "")
    } else {
        warn!("Couldn't find accept-ranges, still trying..")
    }
        

    let mut ranges = (0..size)
        .step_by(chunk_size as usize)
        .map(|from| (from, from + chunk_size - 1))
        .collect::<Vec<_>>();
    ranges.last_mut().expect("Failed getting last range").1 = size;

    debug!("ranges:\n{:?}", ranges);

    let (tx, mut rx) = tokio::sync::mpsc::channel((chunk_size * 4) as usize);

    let start_time = Instant::now();
    let mut downloaders: Vec<JoinHandle<()>> = vec![];
    //downloaders
    for idx in 0..c.chunks {
        let (from, to) = (ranges[idx as usize].0, ranges[idx as usize].1);
        let client = client.clone();
        let url = c.url.clone();
        let tx = tx.clone();
        downloaders.push(tokio::spawn(get_chunk(
            client,
            tx,
            from,
            to,
            url,
            idx as usize,
        )))
    }

    let file_manager = tokio::spawn(file_manager(rx, c, size, start_time));

    for task in downloaders {
        task.await.map_err(|x| error!("{}", x)).unwrap();
    }
    file_manager.await.map_err(|x| error!("{}", x)).unwrap();
    Ok(())
}

async fn file_manager(mut rx: Receiver<Chunk>, c: Cli, size:u64, start_time: Instant) {
    let file_path = get_file_path(&c);

    debug!("Parsed target file path as:\n {:?}", file_path);

    let mut file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .open(&file_path)
        .await
        .expect("Failed opening file");

    file.set_len(size)
        .await
        .expect("Failed setting file length?");

    let mut finished_chunks = Status::new(c.chunks.into());
    //[1|2|3|4|5|6]
    //[x¹|x²|x³|✓¹|x¹|x¹]
    while let Some(cmd) = rx.recv().await {
        match cmd {
            Chunk::Downloaded {
                index,
                offset,
                bytes,
            } => {
                debug!(
                    "Recieved file parts: [{}] [{}->{}",
                    index,
                    offset,
                    bytes.len()
                );
                file.seek(std::io::SeekFrom::Start(offset))
                    .await
                    .expect("Failed seeking into file offset");
                file.write_all(&bytes).await.expect("File write_all failed");
                debug!("written chunk [{}]", index);
                finished_chunks.push(index);
            }
        }
        println!("{}", finished_chunks);
        if finished_chunks.check() {
            let duration = start_time.elapsed();
            println!("Download finished in {:?}", duration);
            break;
        }
    }    
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
enum Chunk {
    Downloaded {
        index: usize,
        offset: u64,
        bytes: Bytes,
    },
}

struct Status {
    total_chunks: usize,
    chunks: HashSet<usize>,
}

impl Status {
    fn new(total_chunks: usize) -> Self {
        Status {
            total_chunks,
            chunks: HashSet::new(),
        }
    }
    #[inline]
    fn push(&mut self, chunk: usize) {
        self.chunks.insert(chunk);
    }
    #[inline]
    fn check_len(&self) -> bool {
        self.chunks.len() == self.total_chunks
    }
    fn check(&self) -> bool {
        if self.check_len(){
            for c in 0..self.total_chunks {
                if !self.chunks.contains(&c) {
                    return false;
                }
            }
            true
        } else {
            false
        }
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[")?;
        for c in (0..self.total_chunks - 1) {
            if self.chunks.contains(&c) {
                let s = format!("✓{:>3}", superscript(c)).green().on_bright_green();
                write!(f, "{}", s)?;
            } else {
                let s = format!("x{:>3}", superscript(c)).red().on_bright_red();
                write!(f, "{}", s)?;
            }
            write!(f, "|")?;
        }
        let c = self.total_chunks - 1;
        if self.chunks.contains(&c) {
            let s = format!("✓{:>3}", superscript(c)).green().on_bright_green();
            write!(f, "{}", s)?;
        } else {
            let s = format!("x{:>3}", superscript(c)).red().on_bright_red();
            write!(f, "{}", s)?;
        }
        write!(f, "]")
    }
}

const supe: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
fn superscript(mut n: usize) -> String {
    let mut s: String = String::new();
    if n == 0 { s.push(supe[0]); s }
    else {
        while n > 0 {
            s.push(supe[n % 10]);
            n = n / 10;
        }
        s.chars().rev().collect()

    }
}
async fn get_chunk(
    client: Client,
    tx: Sender<Chunk>,
    from: u64,
    to: u64,
    url: String,
    index: usize,
) {
    let response = loop {
        let request = client
            .get(&url)
            .header("Range", format!("bytes={}-{}", from, to));
        match request.send().await {
            Err(e) => {
                debug!("Chunk request failed with {}, retrying", e);
            }
            Ok(o) => {
                debug!("Got response: {:?}", o.headers());
                match o.bytes().await {
                    Ok(b) => break b,
                    Err(e) => {
                        debug!("Failed retrieving bytes from response body? {}", e)
                    }
                }
            }
        }
    };

    tx.send(Chunk::Downloaded {
        index,
        offset: from,
        bytes: response,
    })
    .await
    .expect("Failed sending chunk through channel");
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
