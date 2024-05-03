use std::{
    borrow::Borrow,
    io::{Read, Write},
    path::PathBuf,
    ptr,
};

use clap::Parser;
use donldr::{
    download::{self, determine_file_path, Download},
    set_tracing, DResult,
};
use futures::{stream, FutureExt, StreamExt, TryFutureExt};
use reqwest::Client;
use tokio::{
    fs::File,
    task::{JoinHandle, JoinSet},
    time::Instant,
};
use tokio_util::bytes::BufMut;
use tracing::{
    debug, error, info,
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
    let mut downloaders = JoinSet::new();

    let start_time = Instant::now();

    // let stream_of_requests = stream::iter((0..download.info.chunks).map(|idx| {
    //     let (from, to) = download.get_ranges(idx as usize);
    //     let req =
    //     download
    //         .client
    //         .get(&download.url)
    //         .header("Range", format!("bytes={}-{}", from, to))
    //         .send();
    //     req
    //         .then(|x| x.expect("Failed getting request").bytes())
    //         .map_err(|e| (e, req.))
    //         .map(move |x| (idx, x))
    // }));
    // let mut buffer = stream_of_requests.buffer_unordered(download.info.chunks);

    // let mut map_write = JoinSet::new();
    // while let Some((idx, response)) = buffer.next().await {
    //     match response {
    //         Ok(res) => {
    //             let (from, to) = download.get_ranges(idx as usize);
    //             let len = (to - from + 1) as usize;
    //             let mut chunk =
    //             Memory::new(unsafe { mmap.as_mut_ptr().add(from as usize) }, len);
    //             debug!(
    //                 "Finished downloading chunk {}, len:{}, [{from}-{to}]: {}",
    //                 idx,
    //                 res.len(),
    //                 (to - from + 1)
    //             );
    //             map_write.spawn(async move {
    //                 chunk.copy_fill(res.as_ptr(), std::cmp::min(chunk.len, res.len()));
    //                 debug!("Finished writing chunk {}, len:{}", idx, chunk.len);
    //                 idx
    //             });
    //         }
    //         Err((err, req)) => {
    //                 //Request failed how to retry?
    //                 error!("Response to bytes failed: {}", err);
    //                 // Err(idx)
    //                 // buffer.
    //         }
    //     }
    // }
    

    for idx in 0..download.info.chunks {
        let client = download.client.clone();
        let url = download.url.clone();
        let (from, to) = download.get_ranges(idx as usize);
        debug!("range: [{from}-{to}]");
        let len = (to - from + 1) as usize;
        debug!("len  : {len}");
        let mut chunk = Memory::new(unsafe { mmap.as_mut_ptr().add(from as usize) }, len);
        // let mut chunk = mmap.get_mut(from as usize..=to as usize).expect("Slicing mmap failed");

        downloaders.spawn(async move {
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
            debug!(
                "Finished downloading chunk {}, len:{}, [{from}-{to}]: {}",
                idx,
                response.len(),
                (to - from + 1)
            );

            chunk.copy_fill(response.as_ptr(), std::cmp::min(chunk.len, response.len()));
            debug!("Finished writing chunk {}, len:{}", idx, chunk.len);
            // chunk.write_all(response.as_ref());
            drop(chunk);
            idx
        });
    }
    let mut seen: Vec<bool> = vec![false; download.info.chunks];
    while let Some(res) = downloaders.join_next().await {
        let idx = res.unwrap();
        seen[idx] = true;
        let sc = seen.iter().filter(|&x| *x==true).count();
        let perc = (sc as f32 / download.info.chunks as f32) * 100.;
        info!("{}/{}, {:>3}%", sc, download.info.chunks, perc)
    }
    // while let Some(res) = map_write.join_next().await {
    //     let idx = res.unwrap();
    //     seen[idx] = true;
    //     let perc = (seen.iter().count() / download.info.chunks) as f32 * 100.;
    //     info!("{}/{}, {:>3}%", seen.iter().count(), download.info.chunks, perc)
    // }

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
}

impl Memory {
    ///SAFETY:
    /// This type assumes it's a mmap memory region
    /// with required permissions, and a valid len.
    fn new(ptr: *mut u8, len: usize) -> Self {
        Memory { inner: ptr, len }
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
}

unsafe impl Send for Memory {}
unsafe impl Sync for Memory {}
