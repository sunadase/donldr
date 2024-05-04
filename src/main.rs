use std::{
    borrow::Borrow,
    fmt::write,
    io::{Read, Write},
    path::{Display, PathBuf},
    ptr,
    sync::atomic::AtomicUsize,
};

use clap::Parser;
use donldr::{
    download::{self, determine_file_path, Download},
    set_tracing, DResult, Errors,
};
use futures::{stream, FutureExt, StreamExt, TryFutureExt};
use reqwest::{Client, Error};
use tokio::{
    fs::File,
    sync::mpsc::{self, Sender},
    task::{JoinHandle, JoinSet},
    time::Instant,
};
use tokio_util::bytes::BufMut;
use tracing::{
    debug, error, info,
    subscriber::{self, SetGlobalDefaultError},
    warn,
};
use tracing_subscriber::fmt::format;

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
    set_tracing()?;

    let c = Cli::parse();
    debug!("parsed cli:\n{:#?}", c);
    let download = Download::new(c.url, c.path, c.chunks).await?;

    let file_path = determine_file_path(&download.path, &download.url);

    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .open(&file_path)
        .await
        .unwrap();

    file.set_len(download.info.size).await?;

    let mut mmap: memmap2::MmapMut =
        unsafe { memmap2::MmapMut::map_mut(&file).expect("getting a mmap for file failed") };

    // let mut downloaders: Vec<JoinHandle<()>> = vec![];
    let mut downloaders: JoinSet<Result<usize, Errors>> = JoinSet::new();
    let (progress_tx, mut progress_rx) = mpsc::channel(download.info.chunks * 2);

    let start_time = Instant::now();
    let stream_of_requests = stream::iter((0..download.info.chunks).map(|idx| {
        let (from, to) = download.get_ranges(idx as usize);
        let req = download
            .client
            .get(&download.url)
            .header("Range", format!("bytes={}-{}", from, to))
            .send();
        req.map(move |x| (idx, x))
    }));
    let mut buffer = stream_of_requests.buffer_unordered(download.info.chunks);

    while let Some((idx, response)) = buffer.next().await {
        match response {
            Ok(res) => {
                let (from, to) = download.get_ranges(idx as usize);
                let len = (to - from + 1) as usize;
                let mut chunk = Memory::new(unsafe { mmap.as_mut_ptr().add(from as usize) }, len);
                let mut stream = res.bytes_stream();
                let p_tx = progress_tx.clone();
                downloaders.spawn(async move {
                    let mut written = 0;
                    while let Some(chunk_result) = stream.next().await {
                        let respchunk = chunk_result?;
                        chunk.write_at(
                            written,
                            respchunk.as_ptr(),
                            std::cmp::min(respchunk.len(), chunk.len - written),
                        );
                        written += respchunk.len();
                        // let perc = written as f32 / len as f32;
                        // debug!("{}/{} : {:>3}",written, len, perc);
                        p_tx.send((idx, respchunk.len(), len))
                            .await
                            .map_err(|e| Errors::Custom(format!("{:?}", e)))?;
                    }
                    Ok(idx)
                });
            }
            Err((err)) => {
                //Request failed how to retry?
                error!("Send failed: {}", err);
                // Err(idx)
                // buffer.
            }
        }
    }

    let lens = download
        .info
        .ranges
        .iter()
        .map(|(from, to)| (to - from + 1) as usize)
        .collect();
    debug!("lens   {:?}", lens);
    let mut stats = Status::new(lens);
    debug!("stats: {:?}", stats);
    //TODO: indicatif
    tokio::spawn(async move {
        let mut write_checkp = 0;
        let mut total_written = 0;
        while let Some((idx, written, len)) = progress_rx.recv().await {
            stats.add_to(idx, written);
            total_written += written;
            // debug!("[{:2}]: {} of {}", idx, written, len);
            if ((total_written - write_checkp) as f32 / download.info.size as f32) * 100. > 1. {
                write_checkp = total_written;
                info!("\n{}", stats);
            }
        }
        debug!("Progress loop ended");
    });

    while let Some(Ok(res)) = downloaders.join_next().await {
        let idx = res.unwrap();
        info!("done {}", idx);
    }

    debug!("Mem download finished in {:?}", start_time.elapsed());
    debug!("all tasks finished and returned");

    let disk_time = Instant::now();
    mmap.flush()?;
    debug!("mmap flushed, took {:?}", disk_time.elapsed());
    debug!("Total download finished in {:?}", start_time.elapsed());

    Ok(())
}

/// SAFETY:
///  This type's mutating functions are UNSAFE
///  it's suppossed to be used with MMAPd memory
///  address with a known length. If the length
///  provided is not right or ptr is not a valid
///  memory region with required permissions it
///  will cause issues.
struct Memory {
    inner: *mut u8,
    len: usize,
    cursor: usize,
}

impl Memory {
    ///SAFETY:
    /// This type assumes it's a mmap memory region
    /// with required permissions, and a valid len.
    fn new(ptr: *mut u8, len: usize) -> Self {
        Memory {
            inner: ptr,
            len,
            cursor: 0,
        }
    }

    ///SAFETY:
    /// This type assumes it's a mmap memory region
    /// with required permissions.
    /// If the src to be copied from has length
    /// less than self.len this function will try
    /// to access beyond it's length:
    ///     SIGSEGV ADDRESS BOUNDARY ERROR
    /// consider using:
    ///     copy_fill with min(self.len, src.len)
    fn copy_fill_from(&mut self, src: *const u8) {
        unsafe {
            self.inner.copy_from(src, self.len);
        }
    }

    ///SAFETY:
    /// If the len doesnt go out of bounds for both
    /// src and self, assuming this types usecase
    /// as a mmap memory region and src an accessible
    /// memory address, it should be safe.
    /// len should be: min(self.len, src.len)
    fn copy_fill(&mut self, src: *const u8, len: usize) {
        assert!(
            len <= self.len,
            "Can't give a length larger than allocated memory length"
        );
        unsafe { self.inner.copy_from(src, len) }
    }

    fn write_at(&mut self, offset: usize, src: *const u8, len: usize) {
        assert!(
            offset + len <= self.len,
            "Writes out of bounds, offset+len can't be larger than self.len"
        );
        unsafe { self.inner.add(offset).copy_from(src, len) }
    }
}

unsafe impl Send for Memory {}
unsafe impl Sync for Memory {}

#[derive(Debug)]
struct Status {
    progs: Vec<Progress>,
}
impl Status {
    /// total size list must be index ordered
    fn new(total_size_list: Vec<usize>) -> Self {
        Status {
            progs: total_size_list
                .iter()
                .map(|chunk_sz| Progress::new(0, *chunk_sz))
                .collect(),
        }
    }

    fn add_to(&mut self, idx: usize, written: usize) {
        assert!(idx < self.progs.len(), "Indexing beyond existing progs?");
        self.progs.get_mut(idx).unwrap().add_prog(written);
    }

    fn get_total_written(&self) -> usize {
        self.progs.iter().fold(0, |a, b| a + b.current)
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[")?;
        for pg in 0..self.progs.len() - 1 {
            write!(f, "{:^4}|", pg)?;
        }
        write!(f, "|{:^4}]\n", self.progs.len() - 1)?;
        write!(f, "[")?;
        for pg in 0..self.progs.len() - 1 {
            write!(
                f,
                "{:^4}%|",
                self.progs.get(pg).unwrap().percentage() * 100.
            )?;
        }
        write!(
            f,
            "|{:^4}%]",
            self.progs.last().unwrap().percentage() * 100.
        )
    }
}

#[derive(Debug)]
struct Progress {
    current: usize,
    total: usize,
}

impl Progress {
    fn new(current: usize, total: usize) -> Self {
        Progress { current, total }
    }
    fn percentage(&self) -> f32 {
        self.current as f32 / self.total as f32
    }
    fn add_prog(&mut self, written: usize) {
        if self.current + written > self.total {
            // error!("Progress overflow?, got more chunks than expected?: {}/{} wanted to write {} more -> !{}<={}", self.current, self.total, written, written+self.current, self.total);
        }
        // assert!(written+self.current <= self.total, "Progress overflow?, got more chunks than expected");
        self.current += written
    }
}
