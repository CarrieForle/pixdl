use regex::Regex;
use reqwest::Client;
use anyhow::anyhow;
use pixiv::{PixivResource, PixivError};
use thirtyfour::WebDriver;
use twitter::{TwitterResource, TwitterError};

pub(super) mod pkce;
pub(super) mod pixiv;
pub(super) mod twitter;

type ParseResult = std::result::Result<ParsedResource, ParseError>;
type FailedIndexes = Option<Vec<usize>>;

/// The return type of downloading a resource.
/// 
/// Return `Ok(None)` if all subresources are successfully downloaded.
/// 
/// Return `Ok(Some(Vec<usize>))` if some subresources are failed to download. The Vec contains the failed subresources index. Note the index starts from 1.
/// 
/// Return [PixivError] if errors happened before it could start downloading any subresource.
/// 
/// Note invalid subresources due to invalid options would not cause `Ok(Some(Vec<usize>))`.
pub type Result<E> = std::result::Result<FailedIndexes, E>;

#[derive(thiserror::Error, Debug)]
#[error(transparent)]
pub struct ParseError(#[from] anyhow::Error);

#[derive(thiserror::Error, Debug)]
pub enum ResourceError {
    #[error(transparent)]
    Pixiv(#[from] PixivError),

    #[error(transparent)]
    Twitter(#[from] TwitterError),

    #[error("Unsupported")]
    Unknown,
}

// Intermediate resource that has the parsed information.
#[derive(Debug, Clone)]
pub enum ParsedResource {
    Pixiv(ParsedPixivResource),
    Twitter(ParsedTwitterResource),
    Unknown(UnknownResource),
}

#[derive(Debug, Clone)]
pub struct ParsedPixivResource {
    origin: String,
    id: String,
    options: Vec<String>,
}

impl ParsedPixivResource {
    pub fn to_pixiv(self, client: Client) -> PixivResource {
        PixivResource { 
            origin: self.origin, 
            id: self.id.into(), 
            options: self.options, 
            client, 
            metadata: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParsedTwitterResource {
    id: String,
    origin: String,
    url: String,
    options: Vec<String>
}

impl ParsedTwitterResource {
    pub fn to_twitter(self, client: Client, driver: WebDriver) -> TwitterResource {
        TwitterResource { 
            origin: self.origin,
            options: self.options, 
            url: self.url,
            id: self.id.into(),
            driver,
            client
        }
    }
}

#[derive(Debug, Clone)]
pub enum Resource {
    Pixiv(PixivResource),
    Twitter(TwitterResource),
    Unknown(UnknownResource),
}

impl Resource {
    pub fn parse(origin: &str) -> ParseResult {
        let origin = origin.trim().to_string();
        if origin.is_empty() {
            Err(anyhow!("origin is empty"))?
        }

        let pixiv_regex = Regex::new(r"^https:\/\/www\.pixiv\.net\/artworks\/(\d+)\/?").unwrap();
        let twitter_regex = Regex::new(r"^https:\/\/(?:x|twitter)\.com\/\w+?\/status\/(\d+)\/?").unwrap();

        let mut tokens = origin.split_whitespace();
        let url = tokens.next().unwrap();
        let tokens = tokens.map(String::from)
            .collect();
        
        if let Some(captures) = pixiv_regex.captures(url) {
            let id = captures[1].to_string();

            Ok(ParsedResource::Pixiv(ParsedPixivResource {
                origin,
                id,
                options: tokens,
            }))
        } else if let Some(captures) = twitter_regex.captures(url) {
            let url = url.to_string();
            let id = captures[1].to_string();

            Ok(ParsedResource::Twitter(ParsedTwitterResource {
                origin,
                id,
                url,
                options: tokens,
            }))
        } else {
            Ok(ParsedResource::Unknown(origin.into()))
        }
    }

    // pub fn origin(&self) -> &str {
    //     match self {
    //         Resource::Pixiv(pixiv) => pixiv.origin(),
    //         Resource::Twitter(twitter) => twitter.origin(),
    //         Resource::Unknown(unknown) => unknown.origin(),
    //     }
    // }

    // pub async fn download(&mut self) -> self::Result<ResourceError> {
    //     match self {
    //         Resource::Pixiv(pixiv) => Ok(pixiv.download().await?),
    //         Resource::Twitter(twitter) => Ok(twitter.download().await?),
    //         Resource::Unknown(_) => Err(ResourceError::Unknown)
    //     }
    // }
}

pub type Resources = Vec<Resource>;
pub type ParsedResources = Vec<ParsedResource>;

/// Unidentified resource from the user. It does not support
/// download and is skipped upon encounter. This kind of resources
/// remain at input file after program terminates.
#[derive(Debug, Clone)]
pub struct UnknownResource {
    pub(crate) origin: String,
}

impl From<String> for UnknownResource {
    fn from(s: String) -> Self {
        UnknownResource { origin: s }
    }
}

impl UnknownResource {
    pub fn origin(&self) -> &str {
        &self.origin
    }
}