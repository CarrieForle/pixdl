use std::{fs::{self, File, remove_dir_all, remove_file}, io::{BufWriter, Write}, path::Path};
use futures::StreamExt;
use reqwest::{Client, Url, header::HeaderMap};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to write into disk")]
    Io(#[from] std::io::Error),

    #[error("Failed to fetch resource")]
    Network(#[from] reqwest::Error),
}

/// Download blob with configuration
#[derive(Debug)]
pub struct Builder<'p> {
    url: Url,
    client: Client,
    dst: &'p Path,
    headers: Option<HeaderMap>
}

impl<'p> Builder<'p> {
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
            .error_for_status()?
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
        
        while let Some(res) = stream.next().await {
            let chunk;
            (chunk, file) = Self::delete_file_on_error(res, file, self.dst)?;
            byte_count += chunk.len() as u64;
            (_, file) = Self::delete_file_on_error(
                file.write_all(&chunk),
                file,
                self.dst
            )?;
        }

        Ok(byte_count)
    }

    // Close file and delete file/directory on removal.
    // `file` and `dst` MUST be the same location in the filesystem.
    fn delete_file_on_error<T, F: Write, E: Into<Error>>(
        result: Result<T, E>,
        file: BufWriter<F>, 
        dst: &Path, 
    ) -> Result<(T, BufWriter<F>), Error> 
    {
        match result {
            Err(err) => {
                drop(file);
                if dst.is_file() {
                    remove_file(dst)?;
                // TODO: could dst be neither a file or directory? 
                // The else assume it's a directory.
                } else {
                    remove_dir_all(dst)?;
                }

                Err(err.into())
            }
            Ok(res) => {
                Ok((res, file))
            }
        }
    }

    pub fn headers(mut self, headers: HeaderMap) -> Self {
        self.headers = Some(headers);
        self
    }

    pub fn new<P: AsRef<Path>>(client: Client, url: Url, dst: &'p P) -> Self {
        Builder { 
            url, 
            client, 
            dst: dst.as_ref(), 
            headers: None
        }
    } 
}