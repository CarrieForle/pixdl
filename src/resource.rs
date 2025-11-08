use std::{collections::HashSet, fs::{self, File}, io::{self, BufWriter}, ops::Deref, path::PathBuf, sync::Arc};
use regex::Regex;
use reqwest::{Client, IntoUrl, Url, header::HeaderMap};
use anyhow::{Context, anyhow};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use crate::download;

#[derive(thiserror::Error, Debug)]
#[error(transparent)]
pub enum PixivError {
    #[error("Failed to fetch Pixiv illustration")]
    Network(#[from] reqwest::Error),

    #[error("Failed to extract detail from Pixiv JSON response")]
    JsonTraversal,
    
    #[error("Failed to download Pixiv illustration")]
    Download(#[from] download::Error),

    #[error("Failed to parse option")]
    Option,

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<io::Error> for PixivError {
    fn from(e: io::Error) -> PixivError {
        PixivError::Download(download::Error::Io(e))
    }
}

impl From<serde_json::Error> for PixivError {
    fn from(e: serde_json::Error) -> PixivError {
        PixivError::Download(download::Error::Io(e.into()))
    }
}

#[derive(thiserror::Error, Debug)]
#[error(transparent)]
pub struct ParseError(#[from] anyhow::Error);

#[derive(Debug, Clone)]
pub enum Resource {
    Pixiv(PixivResource),
    Unknown(UnknownResource),
}

impl Resource {
    pub fn parse(client: Client, origin: &str) -> Result<Self, ParseError>  {
        if origin.trim().is_empty() {
            Err(anyhow!("origin is empty"))?
        }

        let pixiv_regex = Regex::new(r"^https:\/\/www\.pixiv\.net\/artworks\/(\d+)\/?$").unwrap();
        let origin: Box<str> = Box::from(origin);

        let mut tokens: Vec<_> = origin.split_whitespace()
            .map(Box::from)
            .collect();

        if tokens.is_empty() {
            Err(anyhow!("origin is empty"))?
        }

        let link = tokens.drain(..1).next().unwrap();
        let captures = pixiv_regex.captures(&link);

        if let Some(caps) = captures {
            let id = Arc::from(&caps[1]);
            let tokens = tokens.into_iter()
                .collect();

            Ok(Resource::Pixiv(PixivResource {
                origin,
                id,
                options: tokens,
                client,
                metadata: None,
            }))
        } else {
            Ok(Resource::Unknown(origin.into()))
        }
    }
}

pub type Resources = Vec<Resource>;

// I'm seriously not sure if it's worth it to use Box<str>. But I
// am not going to mutate any of these. It's wrong to mutate 
// this data structure. It might make sense that some of these
// are absense which in case I would make it Option instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PixivMetadata {
    pub artist: Box<str>,
    pub title: Box<str>,
    pub link: Box<str>,
}

#[derive(Debug)]
struct PixivDownloadResource {
    pub id: Arc<str>,
    pub client: Client,
}

impl From<&PixivResource> for PixivDownloadResource {
    fn from(pixiv: &PixivResource) -> Self {
        PixivDownloadResource {
            id: pixiv.id.clone(),
            client: pixiv.client.clone(),
        }
    }
}

impl PixivDownloadResource {
    /// is_subresource is true if there are multiple artwork.
    async fn write_pic_link_to_disk<U>(&self, url: U, is_subresource: bool) -> Result<(), PixivError> 
    where 
        U: IntoUrl, 
    {
        let url = url.into_url()?;

        let filename_from_url = url.path_segments()
            .ok_or(PixivError::Other(anyhow!("URL does not have path")))?
            .next_back()
            .ok_or(PixivError::Other(anyhow!("Failed to extract filename from URL")))?;

        let dst = if is_subresource {
            fs::create_dir_all(self.id.deref())?;
            let mut dst = PathBuf::from(self.id.deref());
            dst.push(filename_from_url);
            dst
        } else {
            let ext: &str = filename_from_url
                .rfind('.')
                .map(|index| &filename_from_url[index..])
                .unwrap_or("");

            PathBuf::from(format!("{}{}", self.id, ext))
        };

        let mut headers = HeaderMap::with_capacity(1);
        headers.append("Referer", "https://www.pixiv.net".parse().unwrap());

        download::Builder::new(self.client.clone(), url, &dst)
            .headers(headers)
            .download()
            .await?;

        Ok(())
    }
}

/// origin is the exact line that create the resource from the input file.
/// id is illustration id
#[derive(Debug, Clone)]
pub struct PixivResource {
    pub(crate) origin: Box<str>,
    pub(crate) id: Arc<str>,
    pub(crate) options: Vec<Box<str>>,
    pub(crate) client: Client,
    
    // This should be None when initialized since all the metadata
    // are supposed to be polluted from [download].
    pub(crate) metadata: Option<PixivMetadata>,
}

impl PixivResource {
    pub fn origin(&self) -> &str{
        &self.origin
    }

    /// Download a (group of) illustration subresourcees by ID.
    /// Return Ok(None) if all subresources are successfully downloaded.
    /// Return Ok(Some(Vec<_>)) if some subresources are failed to download. The Vec contains the failed subresources index. Note the index starts from 1.
    /// Return PixivError if errors happened before it could start downloading any subresource.
    /// 
    /// Note invalid subresources denoted by options would not cause Ok(false).
    pub async fn download(&mut self) -> Result<Option<Vec<usize>>, PixivError> {
        let detail = {
            let url = format!("https://www.pixiv.net/ajax/illust/{}", self.id);
            self.client.get(url)
                .header("Referer", "https://www.pixiv.net/")
                .send().await?
                .error_for_status()?
                .json::<serde_json::Value>().await?
        };

        let detail = &detail["body"];
        let title = detail["title"].as_str()
            .ok_or(PixivError::JsonTraversal)?;
        let artist = detail["userName"].as_str()
            .ok_or(PixivError::JsonTraversal)?;

        self.metadata = Some(PixivMetadata {
            artist: Box::from(artist),
            title: Box::from(title),
            link: Box::from(format!("https://www.pixiv.net/artworks/{}", self.id)),
        });

        let metadata = self.metadata.as_ref().unwrap();
        let tags = detail.pointer("/tags/tags").ok_or(PixivError::JsonTraversal)?
        .as_array()
        .ok_or(PixivError::JsonTraversal)?;

        // if find "動圖" in tag. Use a different download method.
        for tag in tags {
            let tag = tag["tag"].as_str().ok_or(PixivError::JsonTraversal)?;

            if tag == "動圖" {
                return self.download_video().await;
            }
        }

        let pictures = {
            let url = format!("https://www.pixiv.net/ajax/illust/{}/pages", self.id.clone());
            self.client.clone().get(url)
                .header("Referer", "https://www.pixiv.net/")
                .send().await?
                .error_for_status()?
                .json::<serde_json::Value>().await?
        };

        let pictures = pictures["body"].as_array()
            .ok_or(PixivError::JsonTraversal)?;

        // The parsing index starts from 1.
        // Need to convert to start from 0 for accessing resources.
        let mut too_high_indexes: Vec<&str> = Vec::new();
        let download_indexes = match self.options.len() {
            0 => (0..pictures.len()).collect(),
            _ => {
                let range_regex = Regex::new(r"(\d){1,3}\.\.(\d){1,3}").unwrap();
                let mut indexes = HashSet::new();

                for option in &self.options {
                    if let Some(caps) = range_regex.captures(option) {
                        let mut start = caps[1].parse().unwrap();
                        let mut end = caps[2].parse().unwrap();
    
                        if end < start || start == 0 {
                            Err(PixivError::Option).context("Invalid range")?
                        }
    
                        if end > pictures.len() {
                            println!("[Pixiv ({})] Ending range is too large ({}). Adjusted to the the end of illustration ({})", self.id, end, pictures.len());
    
                            end = pictures.len();
                        }
                        
                        start -= 1;
                        end -= 1;
    
                        indexes = indexes.into_iter()
                            .chain(start..=end)
                            .collect();
                    } else {
                        let index: usize = option.parse()
                            .or(Err(PixivError::Option))
                            .context(format!("\"{option}\" is not a number"))?;

                        if index == 0 {
                            Err(PixivError::Option)
                            .context("Number must be positive. Found 0.")?
                        }

                        if index > pictures.len() {
                            too_high_indexes.push(option);
                            continue;
                        }

                        indexes.insert(index - 1);
                    }
                }

                indexes
            }
        };

        if !too_high_indexes.is_empty() {
            let sequence = too_high_indexes.join(", ");
            let sequence = sequence.trim();
            println!("{}", format!("[Pixiv ({})] Skipped indexes ({sequence}) due to exceeding the number of illustration.", self.id).yellow());
        }

        if pictures.len() <= 1 {
            let url = pictures[0].pointer("/urls/original")
            .ok_or(PixivError::JsonTraversal)?.as_str()
            .ok_or(PixivError::JsonTraversal)?;

            PixivDownloadResource::from(&*self).write_pic_link_to_disk(url, false).await?;

            Ok(None)
        } else {
            let mut failed_subresources = Vec::new();
            let mut file_path = PathBuf::from(self.id.deref());
            fs::create_dir_all(&file_path)?;
            file_path.push("metadata.json");
            let metadata_file = BufWriter::new(File::create(file_path)?);

            serde_json::to_writer_pretty(metadata_file, metadata)?;
            let (sender, mut receiver) = mpsc::channel(5);

            for i in download_indexes {
                let url = pictures.get(i)
                    .ok_or(PixivError::Other(anyhow!("Index out of bound")))?
                    .pointer("/urls/original")
                    .ok_or(PixivError::JsonTraversal)?
                    .as_str()
                    .ok_or(PixivError::JsonTraversal)?
                    .to_string();

                let id = self.id.clone();
                let pixiv_downloader = PixivDownloadResource::from(&*self);
                let sender = sender.clone();
                
                tokio::spawn(async move {
                    if let Err(e) = pixiv_downloader
                        .write_pic_link_to_disk(url, true).await 
                    {
                        let context = format!("Failed to download Pixiv illustration (ID: {}, Index: {})", id, i + 1);
                        let error = anyhow::Error::from(e).context(context);
                        println!("{}", format!("{:#}", error).red());
                        sender.send(i + 1).await.unwrap();
                    }
                });
            }

            drop(sender);

            while let Some(sub_id) = receiver.recv().await {
                failed_subresources.push(sub_id);
            }

            failed_subresources.sort();
        
            Ok(if failed_subresources.is_empty() {
                None
            } else {
                Some(failed_subresources)
            })
        }
    }

    /// Video is Ugoira(動圖) which a zip archive of frames.
    /// It either returns Ok(None) or Err(PixivError) because it's
    /// assumed there will only be one video per resource.
    /// 
    /// The return type is made compatible to `download`.
    async fn download_video(&self) -> Result<Option<Vec<usize>>, PixivError> {
        let url = format!("https://www.pixiv.net/ajax/illust/{}/ugoira_meta", self.id);
        let mut headers = HeaderMap::with_capacity(1);
        headers.append("Referer", "https://www.pixiv.net/".parse().unwrap());

        let video = self.client.get(url)
            .headers(headers.clone())
            .send().await?
            .error_for_status()?
            .json::<serde_json::Value>().await?;

        let frame_data = video.pointer("/body/frames")
            .ok_or(PixivError::JsonTraversal)?;

        assert!(self.metadata.is_some());
        let illust_metadata = self.metadata.as_ref().unwrap();
        let metadata = serde_json::to_value(illust_metadata)?;

        let mut file_path = PathBuf::from(self.id.deref());
        fs::create_dir_all(&file_path)?;
        file_path.push("metadata.json");
        let metadata_file = BufWriter::new(File::create(&file_path)?);
        serde_json::to_writer_pretty(metadata_file, &metadata)?;

        file_path.pop();
        file_path.push("frame.json");
        let frame_file = BufWriter::new(File::create(&file_path)?);
        serde_json::to_writer_pretty(frame_file, &frame_data)?;

        let video_archive_url = video.pointer("/body/originalSrc")
            .ok_or(PixivError::JsonTraversal)?
            .as_str()
            .ok_or(PixivError::JsonTraversal)?;

        let ext = video_archive_url.rfind('.')
            .map(|index| &video_archive_url[index..])
            .unwrap_or("");

        let video_archive_url: Url = video_archive_url.parse()
            .map_err(|_| PixivError::Other(anyhow!("Failed to extract extension from URL")))?;

        let mut dst = file_path;
        dst.pop();
        dst.push(format!("{}{}", self.id, ext));

        download::Builder::new(self.client.clone(), video_archive_url, &dst)
            .headers(headers)
            .download()
            .await?;

        Ok(None)
    }
}

/// Unidentified resource from the user. It does not support
/// download and is skipped upon encounter. This kind of resources
/// remain at input file after program terminates.
#[derive(Debug, Clone)]
pub struct UnknownResource {
    pub(crate) origin: Box<str>,
}

impl From<Box<str>> for UnknownResource {
    fn from(s: Box<str>) -> Self {
        UnknownResource { origin: s }
    }
}

impl UnknownResource {
    pub fn origin(&self) -> &str {
        &self.origin
    }
}