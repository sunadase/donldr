use std::{borrow::Borrow, io::{Read, Write}, path::PathBuf, ptr};

use clap::Parser;
use donldr::{download::{self, determine_file_path, Download}, set_tracing, DResult};
use reqwest::Client;
use tokio::{fs::File, task::JoinHandle};
use tokio_util::bytes::BufMut;
use tracing::{
    debug, error, subscriber::{self, SetGlobalDefaultError}, warn
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
        
    let mut downloaders: Vec<JoinHandle<()>> = vec![];
    for idx in 0..download.info.chunks {
        let client = download.client.clone();
        let url = download.url.clone();
        let (from, to) = download.get_ranges(idx as usize);
        debug!("range: [{from}-{to}]");
        let len = (to-from+1) as usize;
        debug!("len  : {len}");
        let mut chunk = Memory::new(unsafe { mmap.as_mut_ptr().add(from as usize) }, len);
        // let mut chunk = mmap.get_mut(from as usize..=to as usize).expect("Slicing mmap failed");

        downloaders.push(tokio::spawn(async move {
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
            debug!("Finished downloading chunk {}, len:{}", idx, response.len());
            
            chunk.copy_fill_from(response.as_ptr());
            debug!("Finished writing chunk {}, len:{}", idx, chunk.len);
            // chunk.write_all(response.as_ref());
            drop(chunk);
        }));
    }
    for task in downloaders {
        task.await.map_err(|x| error!("{}", x)).unwrap();
    }
    debug!("all tasks finished and returned");
    
    mmap.flush()?;
    debug!("mmap flushed");
    // for (idx, chunk) in mmap.chunks_mut(download.info.chunk_size as usize).enumerate() {
    //     let client = download.client.clone();
    //     let url = download.url.clone();
    //     let (from, to) = download.get_ranges(idx);
    //     tokio::spawn(async move {
    //         let response = loop {
    //             let request = client
    //                 .get(url)
    //                 .header("Range", format!("bytes={}-{}", from, to));
    //             match request.send().await {
    //                 Err(e) => {
    //                     debug!("Chunk request failed with {}, retrying", e);
    //                 }
    //                 Ok(o) => {
    //                     debug!("Got response: {:?}", o.headers());
    //                     match o.bytes().await {
    //                         Ok(b) => break b,
    //                         Err(e) => {
    //                             debug!("Failed retrieving bytes from response body? {}", e)
    //                         }
    //                     }
    //                 }
    //             }
    //         };
        
    //         chunk.write_all(response.as_ref());
    //     });
    // }

    Ok(())
}



struct Memory {
    inner: *mut u8,
    len: usize
}

impl Memory {
    fn new(ptr: *mut u8, len:usize) -> Self {
        Memory { inner: ptr, len}
    }

    fn copy_fill_from(&mut self, src: *const u8) {
        unsafe { self.inner.copy_from(src, self.len); }
    }
}

unsafe impl Send for Memory {}
unsafe impl Sync for Memory {}