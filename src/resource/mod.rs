use std::sync::Arc;
use regex::Regex;
use reqwest::Client;
use anyhow::anyhow;
use crate::pixiv::PixivResource;

pub(super) mod pkce;
pub(super) mod pixiv;

#[derive(thiserror::Error, Debug)]
#[error(transparent)]
pub struct ParseError(#[from] anyhow::Error);

#[derive(Debug, Clone)]
pub enum Resource {
    Pixiv(PixivResource),
    Unknown(UnknownResource),
}

impl Resource {
    pub fn parse(client: Client, origin: &str) -> Result<Self, ParseError> {
        if origin.trim().is_empty() {
            Err(anyhow!("origin is empty"))?
        }

        let pixiv_regex = Regex::new(r"^https:\/\/www\.pixiv\.net\/artworks\/(\d+)\/?$").unwrap();
        let origin: Box<str> = Box::from(origin);

        let mut tokens: Vec<_> = origin.split_whitespace()
            .map(Box::from)
            .collect();

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