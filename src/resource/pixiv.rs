// Method inspired by ZipFile (https://gist.github.com/ZipFile/c9ebedb224406f4f11845ab700124362)
// Certain illustrations require login account to access.
// Pixiv uses OAuth but it's only used internally (i.e., phone app).
use std::{collections::HashSet, fs::{self, File}, io::{self, BufWriter, BufReader, Write, stdin, stdout}, mem, ops::Deref, path::{Path, PathBuf}, sync::Arc};
use regex::Regex;
use reqwest::{Client, IntoUrl, Request, Response, Url, header::{self, HeaderMap, HeaderValue}};
use anyhow::{Context, anyhow};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use md5::{Digest, Md5};
use time::{UtcDateTime, macros::format_description};
use crate::download;
use crate::pkce;

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

    #[error("Login failed")]
    Login(#[from] LoginError),

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
pub enum LoginError {
    #[error("User cancelled login process")]
    Cancelled,

    #[error("Failed to extract access or refresh tokens")]
    JsonTraversal,

    #[error("Network error")]
    Network(#[from] reqwest::Error),

    #[error("Failed to write login credential")]
    IOWrite(#[source] io::Error),
    
    #[error("Failed to read login credential")]
    IORead(#[source] io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone)]
pub struct PixivUser {
    client: Client,
    credential: PixivCredential,
}

const CREDENTIAL_FILENAME: &str = "login.json";
const HASH_SECRET: &str = "28c1fdd170a5204386cb1313c7077b34f83e4aaf4aa829ce78c231e05b0bae2c";
const ORIGIN: &str = "https://app-api.pixiv.net/web/v1/login";
const AUTH_TOKEN_URL: &str = "https://oauth.secure.pixiv.net/auth/token";
const CLIENT_ID: &str = "MOBrBDS8blbauoSck0ZfDbtuzpyT";
const CLIENT_SECRET: &str = "lsACyCD94FhDUtGTXi3QzcFE2uU1hqtDaKeqrdwj";
const REDIRECT_URI: &str = "https://app-api.pixiv.net/web/v1/users/auth/pixiv/callback";

impl PixivUser {
    pub async fn get_illust_urls(&mut self, id: &str) -> Result<Vec<String>, PixivError> {
        let detail: serde_json::Value = {
            let url = "https://app-api.pixiv.net/v1/illust/detail";
            let request = self.client.get(url)
                .query(&[("illust_id", id)])
                .header(header::USER_AGENT, "PixivIOSApp/7.13.3 (iOS 14.6; iPhone13,2)")
                .header(header::HOST, "app-api.pixiv.net")
                .header("app-os", "ios")
                .header("app-os-Version", "14.6")
                .build()?;

            self.retry_if_unauthorized(request).await?.json().await?
        };

        detail.pointer("/illust/meta_pages")
            .ok_or(PixivError::JsonTraversal)?
            .as_array()
            .ok_or(PixivError::JsonTraversal)?
            .iter()
            .map(|v| { 
                Ok(v.pointer("/image_urls/original")
                    .ok_or(PixivError::JsonTraversal)?
                    .as_str()
                    .ok_or(PixivError::JsonTraversal)?
                    .to_string())
            })
            .collect::<Result<Vec<String>, _>>()
    }

    /// Initialize PixivUser with interactive login.
    async fn login(client: Client) -> Result<Self, LoginError> {
        use anyhow::Error;

        let (code_verifier, code_challenge) = pkce::generate();
        let url = format!("{ORIGIN}?code_challenge={code_challenge}&code_challenge_method=S256&client=pixiv-android");

        println!("{url}");
        println!("Access the URL above and log in. You need to extract code after login. Here's how:");
        println!("1. Open DevTool and go to \"Network\" tab.");
        println!("2. Turn on \"Persist Logs\"");
        println!("3. Type \"callback?\" in the filter");
        println!("4. Log in to Pixiv");
        println!("5. Copy the URL that appears back to the console and hit enter.");
        println!("Note: Code's lifetime is extremely short. If the code expires you have to restart the login process.");
        println!();
        print!("Put the URL here = ");
        stdout().flush().unwrap();

        let mut oauth_code_url = String::new();
        stdin().read_line(&mut oauth_code_url)
            .map_err(|e| LoginError::Other(Error::from(e).context("stdin failed")))?;

        if oauth_code_url.trim().is_empty() {
            Err(LoginError::Cancelled)?;
        }

        let code = if oauth_code_url.starts_with("https://") {
            let url = Url::parse(&oauth_code_url)
                .map_err(|e| LoginError::Other(Error::from(e).context("Failed to parse URL")))?;
            let mut queries = url.query_pairs();

            queries.find_map(|(key, value)| {
                if key == "code" {
                    Some(value)
                } else {
                    None
                }
            }).ok_or(LoginError::Other(anyhow!("Failed to retrieve code from URL")))?
            .into_owned()
        } else {
            oauth_code_url
        };

        let response = client.post(AUTH_TOKEN_URL)
            .form(&[
                ("client_id", CLIENT_ID),
                ("client_secret", CLIENT_SECRET),
                ("code", &code),
                ("code_verifier", &code_verifier),
                ("grant_type", "authorization_code"),
                ("include_policy", "true"),
                ("redirect_uri", REDIRECT_URI),
            ])
            .header("User-Agent", "PixivAndroidApp/5.0.234 (Android 11; Pixel 5)")
            .send().await?
            .error_for_status()?;

        let js: serde_json::Value = response.json().await?;
        let access_token = js["access_token"].as_str()
            .ok_or(LoginError::JsonTraversal)?
            .to_string();

        let refresh_token = js["refresh_token"].as_str()
            .ok_or(LoginError::JsonTraversal)?
            .to_string();

        let credential = PixivCredential {
            access_token, 
            refresh_token,
        };

        credential.write_to_disk(CREDENTIAL_FILENAME)
            .map_err(LoginError::IORead)?;

        Ok(PixivUser { 
            client, 
            credential 
        })
    }

    /// https://github.com/upbit/pixivpy/blob/b5707c7d057938aafbe7325d8aa98bb2a1bc7ab3/pixivpy3/api.py#L118
    /// The is a reimplementatino of pixivpy3 without username and password
    async fn refresh_all_tokens(&mut self) -> Result<(), LoginError> {
        let mut headers = HeaderMap::new();
        let localtime = UtcDateTime::now();
        let format = format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]+00:00");
        let localtime = localtime.format(format)
            .expect("Failed to get localtime");
        headers.insert("x-client-time", localtime.parse().unwrap());

        let hash = base16ct::lower::encode_string(&Md5::digest(format!("{}{}", localtime, HASH_SECRET)));

        headers.insert("x-client-hash", hash.parse().unwrap());
        headers.insert("app-os", "ios".parse().unwrap());
        headers.insert("app-os-Version", "14.6".parse().unwrap());
        headers.insert(header::USER_AGENT, "PixivIOSApp/7.13.3 (iOS 14.6; iPhone13,2)".parse().unwrap());
        headers.insert(header::HOST, "oauth.secure.pixiv.net".parse().unwrap());
        
        let response: serde_json::Value = self.client.post("https://oauth.secure.pixiv.net/auth/token")
            .headers(headers)
            .form(&[
                ("get_secure_url", "1"),
                ("client_id", CLIENT_ID),
                ("client_secret", CLIENT_SECRET),
                ("grant_type", "refresh_token"),
                ("refresh_token", &self.credential.refresh_token)
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        self.credential.access_token = response["access_token"]
            .as_str()
            .ok_or(LoginError::JsonTraversal)?
            .to_string();

        self.credential.refresh_token = response["refresh_token"]
            .as_str()
            .ok_or(LoginError::JsonTraversal)?
            .to_string();

        self.credential.write_to_disk(CREDENTIAL_FILENAME)
            .map_err(LoginError::IORead)?;

        Ok(())
    }

    /// request_builder do not have to be called with bearer_auth
    async fn retry_if_unauthorized(&mut self, request: Request) -> Result<Response, LoginError> {
        let mut header_value: HeaderValue = format!("Bearer {}", self.credential.access_token)
            .parse()
            .unwrap();
        
        header_value.set_sensitive(true);
        let mut first_request = request.try_clone().unwrap();
        first_request.headers_mut().insert(header::AUTHORIZATION, header_value);
        // Panic if request body is stream which 
        // shouldn't be possible as stream feature 
        // is not opted in.
        match self.client.execute(first_request)
            // This is not part of the retry condition as it's
            // a network error rather than unauthorized access.
            .await?
            .error_for_status()
        {
            Ok(val) => Ok(val),
            Err(_) => {
                self.refresh_or_login().await?;
                let mut header_value: HeaderValue = format!("Bearer {}", self.credential.access_token)
                    .parse()
                    .unwrap();

                header_value.set_sensitive(true);
                let mut second_request = request;
                second_request.headers_mut().insert(header::AUTHORIZATION, header_value);
                Ok(
                    self.client.execute(second_request)
                    .await?
                    .error_for_status()?
                )
            }
        }
    }

    async fn refresh_or_login(&mut self) -> Result<(), LoginError> {
        if self.refresh_all_tokens().await.is_err() {
            *self = Self::login(self.client.clone()).await?;
        }

        Ok(())
    }

    /// Initialize PixivUser from JSON.
    fn from_disk<P: AsRef<Path>>(client: Client, file_path: P) -> Result<Self, LoginError> {
        let credential = PixivCredential::from_disk(file_path)
            .map_err(LoginError::IORead)?;

        Ok(PixivUser { 
            client,
            credential,
        })
    }

    /// Try to initialize from disk json, otherwise let user login.
    pub async fn init(client: Client) -> Result<PixivUser, LoginError> {
        if let Ok(user) = Self::from_disk(client.clone(), CREDENTIAL_FILENAME) {
            Ok(user)
        } else {
            Self::login(client).await
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PixivCredential {
    pub access_token: String,
    pub refresh_token: String,
}

impl PixivCredential {
    pub fn write_to_disk<P: AsRef<Path>>(&self, file_path: P) -> io::Result<()> {
        let file = BufWriter::new(File::create(file_path)?);
        Ok(serde_json::to_writer_pretty(file, &self)?)
    }

    pub fn from_disk<P: AsRef<Path>>(file_path: P) -> io::Result<Self> {
        let file = BufReader::new(File::open(file_path)?);
        Ok(serde_json::from_reader(file)?)
    }
}


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
pub(crate) struct PixivDownloadResource {
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
    pub(crate) async fn write_pic_link_to_disk<U>(&self, url: U, is_subresource: bool) -> Result<(), PixivError> 
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
        headers.append(header::REFERER, "https://www.pixiv.net".parse().unwrap());

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

        // if find "ugoira" in filename. Use a different download method.
        let original_url = detail.pointer("/urls/original")
            .ok_or(PixivError::JsonTraversal)?;
        let is_require_account = original_url.is_null();

        if !is_require_account && original_url
            .as_str()
            .ok_or(PixivError::JsonTraversal)?
            // The url is supposed to have the pattern:
            // https://i.pximg.net/img-original/img/{date}/{random}/{id}_ugoira0.{ext}
            .contains("ugoira") 
        {
            return self.download_video().await;
        }

        let mut pictures = if is_require_account {
            let mut pixiv_user = PixivUser::init(self.client.clone()).await?;
            pixiv_user.get_illust_urls(&self.id).await?
        } else {
            let url = format!("https://www.pixiv.net/ajax/illust/{}/pages", self.id);

            let response: serde_json::Value = self.client.get(url)
                .header("Referer", "https://www.pixiv.net/")
                .send().await?
                .error_for_status()?
                .json().await?;

            response["body"].as_array()
                .ok_or(PixivError::JsonTraversal)?
                .iter()
                .map(|v| { 
                    Ok::<std::string::String, PixivError>(v.pointer("/urls/original")
                        .ok_or(PixivError::JsonTraversal)?
                        .as_str()
                        .ok_or(PixivError::JsonTraversal)?
                        .to_string())
                })
                .collect::<Result<_, _>>()?
        };

        // The parsing index starts from 1.
        // Need to convert to start from 0 for accessing resources.
        let mut too_high_indexes = Vec::new();
        let download_indexes = if self.options.is_empty() {
            (0..pictures.len()).collect()
        } else {
            let range_regex = Regex::new(r"(\d{1,3})\.\.(\d{1,3})").unwrap();
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
                        too_high_indexes.push(option.deref());
                        continue;
                    }
                    
                    indexes.insert(index - 1);
                }
            }
            
            indexes
        };
        
        if !too_high_indexes.is_empty() {
            let sequence = too_high_indexes.join(", ");
            let sequence = sequence.trim();
            println!("{}", format!("[Pixiv ({})] Skipped indexes ({sequence}) due to exceeding the number of illustration.", self.id).yellow());
        }

        if pictures.len() <= 1 {
            let url = pictures.first()
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
                let url = mem::take(&mut pictures[i]);

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
        headers.append(header::REFERER, "https://www.pixiv.net/".parse().unwrap());

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