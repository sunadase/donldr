use tracing::subscriber::{self, SetGlobalDefaultError};

pub mod download {
    use std::path::PathBuf;

    use reqwest::{Client, Response};
    use tracing::{debug, error, warn};

    use crate::Errors;

    pub struct Info {
        pub headers: Response,
        pub chunks: usize,

        /// content-length
        pub size: u64,
        pub chunk_size: u64,
        pub ranges: Vec<(u64, u64)>,
    }

    impl Info {
        fn new(headers: Response, chunks: usize) -> Self {
            let size = headers
                .headers()
                .get("content-length")
                .expect("Failed to get content length")
                .to_str()
                .unwrap()
                .parse::<u64>()
                .expect("Failed parsing content-length");
            debug!("size: {}", size);
            let chunk_size = size / chunks as u64;
            //? if chunks>size?
            debug!("chunk size: {}", chunk_size);
            let mut ranges = (0..size)
                .step_by(chunk_size as usize)
                .map(|from| (from, from + chunk_size - 1))
                .collect::<Vec<_>>();
            ranges.last_mut().expect("Failed getting last range").1 = size;
            debug!("ranges:\n{:?}", ranges);

            Info {
                headers,
                chunks,
                size,
                chunk_size,
                ranges,
            }
        }
        pub fn check_accept_ranges(&self) -> bool {
            match self
                .headers
                .headers()
                .get("accept-ranges")
                .map(|x| x.to_str())
            {
                Some(Ok(accept_ranges)) => {
                    debug!("accept ranges: {:?}", accept_ranges);
                    match accept_ranges {
                        "none" => false,
                        _ => true,
                    }
                }
                Some(Err(_)) => {
                    error!("Failed converting accept-ranges header to str. aborting");
                    false
                }
                None => {
                    warn!("Couldn't find accept-ranges, trying chunked download regardless..");
                    true
                }
            }
        }
    }

    pub struct Download {
        pub client: Client,
        pub url: String,
        pub path: String,
        pub info: Info,
    }

    impl Download {
        pub async fn new<S: AsRef<str>>(url: S, path: S, chunks: usize) -> Result<Self, Errors> {
            let url = reqwest::Url::parse(url.as_ref()).expect("Failed parsing url");
            debug!("parsed url:\n{:#?}", &url);

            let client = reqwest::Client::new();
            if let Ok(headers) = client.head(url.as_str()).send().await {
                debug!("headers at target url:\n{:#?}", headers);
                let info = Info::new(headers, chunks);

                if info.check_accept_ranges() {
                    Ok(Download {
                        client,
                        url: url.as_str().to_owned(),
                        path: path.as_ref().to_owned(),
                        info,
                    })
                } else {
                    Err(
                        "Target url doesn't have a valid accept-range header for chunked requests"
                            .into(),
                    )
                }
            } else {
                Err("Failed getting headers".into())
            }
        }

        pub fn get_ranges(&self, idx: usize) -> (u64, u64) {
            (
                self.info.ranges[idx as usize].0,
                self.info.ranges[idx as usize].1,
            )
        }
    }

    pub fn determine_file_path(path: &str, url: &str) -> PathBuf {
        let mut p = PathBuf::from(path);
        let filename = match url.rsplit_once("/") {
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
}

#[derive(Debug)]
pub enum Errors {
    Tracing(tracing::subscriber::SetGlobalDefaultError),
    Io(std::io::Error),
    Reqwest(reqwest::Error),
    Custom(String),
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
impl From<&str> for Errors {
    fn from(value: &str) -> Self {
        Errors::Custom(value.to_owned())
    }
}

pub type DResult<T> = Result<T, Errors>;

pub fn set_tracing() -> Result<(), SetGlobalDefaultError> {
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
    Ok(())
}
