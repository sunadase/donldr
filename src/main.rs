use std::{borrow::Borrow, path::PathBuf};

use clap::Parser;
use donldr::{download::{self, determine_file_path, Download}, set_tracing, DResult};
use reqwest::Client;
use tokio::fs::File;
use tokio_util::bytes::BufMut;
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
    chunks: usize,
}

#[tokio::main]
async fn main() -> DResult<()> {
    set_tracing()?;

    let c = Cli::parse();
    debug!("parsed cli:\n{:#?}", c);
    let download = Download::new(c.url, c.path, c.chunks).await?;
    drop(c);

    let file_path = determine_file_path(download.path, download.url);

    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .open(&file_path)
        .await
        .unwrap();

    file.set_len(download.info.size).await?;

    let mut mmap =
        unsafe { memmap2::MmapMut::map_mut(&file).expect("getting a mmap for file failed") };
    // tokio::pin!(mmap);
    mmap.mmap
        .chunks_mut(chunk_size as usize)
        .enumerate()
        .for_each(move |(idx, mem_chunk)| {
            tokio::spawn(chunk_download_write(
                client.clone(),
                idx,
                mem_chunk,
                chunk_size.try_into().unwrap(),
                url.to_string(),
            ));
        });

    // for (idx, (from, to)) in ranges.iter().enumerate() {
    //     let mem_chunk = mmap.range_writer(*from as usize, (to-from+1) as usize).expect("Failed chunking mmap to writer");
    //     tokio::spawn(chunk_download_write(client.clone(), idx, mem_chunk, (from.to_owned(), to.to_owned()), c.url.to_owned()));
    // }

    Ok(())
}

async fn chunk_download_write(
    client: Client,
    idx: usize,
    mem_chunk: &mut [u8],
    chunk_size: usize,
    url: String,
) -> DResult<()> {
    // are chunks mut and manual ranges matching? probably not

    //idx*chunk_size, idx*chunk_size+chunk_size

    let (from, to) = (idx * chunk_size, idx * chunk_size + chunk_size);

    let chunk = client
        .get(url)
        .header("Range", format!("bytes={}-{}", from, to))
        .send()
        .await?
        .error_for_status()?;

    debug!("chunk {} - {:?}:\n{:?}", idx, chunk_size, &chunk);

    // let download_stream = chunk.bytes_stream()
    // .map_err(|e| futures::io::Error::new(futures::io::ErrorKind::Other, e))
    // .into_async_read();

    // let download_stream = download_stream.compat();

    // tokio::io::copy(&mut download_stream, &mut mem_chunk)

    Ok(())
}
