use std::{fs::{self, File}, io::{BufWriter, Write}, path::Path};
use futures_util::StreamExt;
use reqwest::{Client, Url, header::HeaderMap};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to write into disk")]
    Io(#[from] std::io::Error),

    #[error("Failed to fetch resource")]
    Network(#[from] reqwest::Error),
}

/// Download blob with configuration
pub struct Builder<'c, 'p> {
    url: Url,
    client: &'c Client,
    dst: &'p Path,
    headers: Option<HeaderMap>
}

impl<'c, 'p> Builder<'c, 'p> {
    /// Write a remote blob from the network into the disk
    /// 
    /// Return the number of written byte on success. Otherwise 
    /// throws an [Error] object.
    pub async fn download(self) -> Result<u64, Error> {
        let mut request = self.client.get(self.url);

        if let Some(headers) = self.headers {
            request = request.headers(headers);
        }

        let mut stream = request.send().await?
            .bytes_stream();

        if let Some(parent) = self.dst.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = BufWriter::new(
            File::options()
                .write(true)
                .create_new(true)
                .open(self.dst)?
        );
        let mut byte_count: u64 = 0;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            byte_count += chunk.len() as u64;
            file.write_all(&chunk)?;
        }

        Ok(byte_count)
    }

    pub fn headers(mut self, headers: HeaderMap) -> Self {
        self.headers = Some(headers);
        self
    }

    pub fn new<P: AsRef<Path>>(client: &'c Client, url: Url, dst: &'p P) -> Self {
        Builder { 
            url, 
            client, 
            dst: dst.as_ref(), 
            headers: None
        }
    } 
}