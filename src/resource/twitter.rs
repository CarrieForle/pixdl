use std::{fs, io, path::PathBuf, sync::Arc, time::Duration};
use futures::{StreamExt, stream::FuturesOrdered};
use reqwest::{Client, Url};
use thirtyfour::{By, WebDriver, WebElement, error::WebDriverError, prelude::ElementQueryable};
use thiserror;
use colored::Colorize;

use crate::download;

#[derive(thiserror::Error, Debug)]
pub enum TwitterError {
    #[error("Selenium error")]
    Selenium(#[from] WebDriverError),

    #[error("Failed to scrape content")]
    Scraping,

    #[error("Failed to download Twitter object")]
    Download(#[from] download::Error),

    #[error("No attached image")]
    NoImage,
}

impl From<io::Error> for TwitterError {
    fn from(e: io::Error) -> Self {
        Self::Download(download::Error::Io(e))
    }
}

#[derive(Debug, Clone)]
pub struct TwitterResource {
    pub origin: String,
    pub url: String,
    pub id: Arc<str>,
    pub options: Vec<String>,
    pub driver: WebDriver,
    pub client: Client,
}

impl TwitterResource {
    pub fn origin(&self) -> &str {
        todo!()
    }

    pub async fn download(&self) -> super::Result<TwitterError> {
        self.driver.set_window_rect(0, 0, 300, 800).await?;
        self.driver.goto(&self.origin).await?;
        let elems = self.driver.query(By::Css(r#"img[src^="https://pbs.twimg.com/media/"]"#))
            .wait(Duration::from_secs(8), Duration::from_millis(500))
            .all_from_selector().await?;
        
        println!("DEBUG: Found {} elements.", elems.len());

        if elems.is_empty() {
            Err(TwitterError::NoImage)?
        }

        if elems.len() == 1 {
            let elem = &elems[0];
            fetch_and_download(elem, Arc::clone(&self.id), PathBuf::new(), self.client.clone(), None).await?;

            return Ok(None);
        }

        fs::create_dir_all(&*self.id)?;
        let dst = PathBuf::from(&*self.id);
        let mut tasks = FuturesOrdered::new();

        for (i, elem) in elems.into_iter().enumerate() {
            let dst = dst.clone();
            let id = Arc::clone(&self.id);
            let client = self.client.clone();

            tasks.push_back(tokio::spawn( async move {
                if let Err(e) = fetch_and_download(&elem, Arc::clone(&id), dst, client, Some(i)).await {
                    let context = format!("Failed to download Twitter (ID: {}, Index: {})", id, i + 1);
                    let error = anyhow::Error::from(e).context(context);
                    println!("{}", format!("{:#}", error).red());
                    Some(i)
                } else {
                    None
                }
            }));
        }

        let failed_subresources: Vec<_> = tasks.collect::<Vec<_>>().await
            .into_iter()
            .filter_map(|res| res.unwrap())
            .collect();

        self.driver.close_window().await?;

        if failed_subresources.is_empty() {
            Ok(None)
        } else {
            Ok(Some(failed_subresources))
        }
    }
}

async fn fetch_and_download(elem: &WebElement, id: Arc<str>, mut dst: PathBuf, client: Client, index: Option<usize>) -> Result<(), TwitterError> {
    let url: Url = elem.attr("src").await?
        .ok_or(TwitterError::Scraping)?
        .parse()
        .or(Err(TwitterError::Scraping))?;

    let mut ext = None;

    for (key, val) in url.query_pairs() {
        if key == "format" {
            ext = Some(val);
            break;
        }
    }

    let ext = ext.ok_or(TwitterError::Scraping)?;

    if let Some(i) = index {
        dst.push(format!("{}_p{}.{}", id, i, ext));
    } else {
        dst.push(format!("{}.{}", id, ext));
    }
    
    download::Builder::new(client, url, &dst)
        .download().await?;

    Ok::<(), TwitterError>(())
}