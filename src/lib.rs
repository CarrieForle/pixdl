use regex::Regex;
use reqwest::{Client, ClientBuilder};
use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufWriter, ErrorKind, Write};
use std::path::Path;
use std::time::Duration;
use colored::Colorize;
use crate::resource::*;

pub mod resource;
pub mod download;
pub mod global;

pub fn read_input_file<'a, P: AsRef<Path>>(file_path: P, client: &'a Client) -> io::Result<Resources<'a>> {
    let pixiv_regex = Regex::new(r"^https:\/\/www\.pixiv\.net\/artworks\/(\d+)\/?$").unwrap();

    let file = GeneralOpen.open(file_path)?;
    let reader = io::BufReader::new(file);
    let mut resources = Resources::default();

    for origin in reader.lines() {
        let origin = origin?;

        if origin.trim().is_empty() {
            continue;
        }

        let mut tokens: Vec<_> = origin.split_whitespace()
            .map(Box::from)
            .collect();

        if tokens.is_empty() {
            continue;
        }

        let link = tokens.drain(..1).next().unwrap();
        let captures = pixiv_regex.captures(&link);

        if let Some(caps) = captures {
            let id = Box::from(&caps[1]);
            let tokens = tokens.into_iter()
                .collect();

            resources.push(Resource::Pixiv(PixivResource {
                origin: Box::from(origin),
                id,
                options: tokens,
                client,
                metadata: None,
            }));
        } else {
            resources.push(Resource::Unknown(origin.into()));
        }
    }

    Ok(resources)
}

pub trait DefaultOpen {
    fn default_text(&self) -> &'static str;

    fn open<P: AsRef<Path>>(&self, file_path: P) -> io::Result<File> {
        let file = File::open(&file_path);

        if let Err(e) = file {
            if let ErrorKind::NotFound = e.kind() {
                {
                    let mut file = BufWriter::new(File::create_new(&file_path)?);
                    file.write_all(self.default_text().as_bytes())?;
                }

                return File::open(&file_path);
            } else {
                return Err(e);
            }
        }

        file
    }
}

struct GeneralOpen;

impl DefaultOpen for GeneralOpen {
    fn default_text(&self) -> &'static str {
        ""
    }

    fn open<P: AsRef<Path>>(&self, file_path: P) -> io::Result<File> {
        let file = File::open(&file_path);

        if let Err(e) = file {
            if let ErrorKind::NotFound = e.kind() {
                {
                    let _ = File::create_new(&file_path)?;
                }

                return File::open(&file_path);
            } else {
                return Err(e);
            }
        }

        file
    }
}

pub async fn run<P: AsRef<Path>>(file_path: P) -> anyhow::Result<()> {
    let client = ClientBuilder::new()
        .gzip(true)
        .read_timeout(Duration::from_secs(2))
        .connect_timeout(Duration::from_secs(3))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:144.0) Gecko/20100101 Firefox/144.0")
        .build()?;

    let mut resources = read_input_file(&file_path, &client)?;

    if resources.is_empty() {
        println!("No resources are loaded. Open {:?} and put in the resources!", file_path.as_ref());

        let current_exe = env::current_exe()?;
        let binary_name = current_exe.file_name()
            .map(|f| f.to_str().unwrap_or("pxdlp"))
            .unwrap();

        println!("See program usage with \"{} -h\".", binary_name);

        return Ok(());
    }

    let mut failed_resources  = Vec::new();

    println!("Loaded {} resources.", resources.len());   

    for res in &mut resources {
        match res {
            Resource::Pixiv(pixiv) => {
                match pixiv.download().await {
                    Err(e) => {
                        let context = format!("[Pixiv ({})] Failed", pixiv.id);
                        println!("{}", format!("{:#}", anyhow::Error::from(e).context(context)).red());
                        failed_resources.push(pixiv.origin());
                    }
                    Ok(Some(failed_subresources)) => {
                        let sub_id_sequence = failed_subresources.into_iter()
                            .map(|v| v.to_string())
                            .collect::<Box<[String]>>()
                            .join(", ");

                        let title = &pixiv.metadata.as_ref().unwrap().title;

                        println!("[Pixiv {title} (ID: {id}, Sub ID: {sub_id})] {status}", 
                            id=pixiv.id, 
                            sub_id=sub_id_sequence, 
                            status="Failed".red()
                        );
                        failed_resources.push(pixiv.origin());
                    }
                    Ok(None) => {
                        let title = &pixiv.metadata.as_ref().unwrap().title;

                        println!("[Pixiv {title} ({id})] {status}", 
                            id=pixiv.id, 
                            status="Succeeded".green()
                        );
                    }
                }
            }
            Resource::Unknown(unknown) => {
                println!("[Unknown ({})] Skipped", unknown.origin());
                failed_resources.push(unknown.origin());
            }
        }
    }

    if failed_resources.is_empty() {
        println!("{}", "All resources have been successfully downloaded!".green());
    } else {
        let filename = file_path.as_ref().file_name().unwrap();
        println!("{}", format!("Some resources are failed to download or skipped which remains in {:?}.", filename).yellow())
    }

    let mut input_file = BufWriter::new(File::create(&file_path)?);
    input_file.write_all(failed_resources.join("\n").as_bytes())?;

    Ok(())
}