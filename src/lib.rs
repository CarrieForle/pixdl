use anyhow::Context;
use reqwest::ClientBuilder;
use tokio::time::sleep;
use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufWriter, ErrorKind, Write, stdin};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio::sync::mpsc::{self};
use colored::Colorize;
use crate::resource::*;

pub mod resource;
pub mod download;
pub mod global;
pub mod command_line;

pub struct KillOnDropProcess(Child);
impl Drop for KillOnDropProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

pub fn read_input_file<P: AsRef<Path>>(file_path: P) -> anyhow::Result<ParsedResources> {
    let file = GeneralOpen.open(file_path)?;
    let reader = io::BufReader::new(file);
    let mut resources = ParsedResources::default();

    for origin in reader.lines() {
        let origin = origin?;
        if origin.trim().is_empty() {
            continue;
        }

        resources.push(Resource::parse(&origin)?);
    }

    Ok(resources)
}

trait DefaultOpen {
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

pub async fn run<P: AsRef<Path>>(input_file_path: P, arg_resources: ParsedResources) -> anyhow::Result<()> {
    let (resources, is_interactive) = if arg_resources.is_empty() {
        let res = read_input_file(&input_file_path)?;

        if res.is_empty() {
            println!("No resources are loaded. Open {:?} and put in the resources!", input_file_path.as_ref());

            let current_exe = env::current_exe()?;
            let binary_name = current_exe.file_name()
                .map(|f| f.to_str().unwrap_or("pxdlp"))
                .unwrap();

            println!("See program usage with \"{} -h\".", binary_name);

            return Ok(());
        }
    
        println!("Loaded {} resources from {:?}", res.len(), input_file_path.as_ref());
        (res, true)
    } else {
        println!("Loaded {} resources from command line arguments", arg_resources.len());
        (arg_resources, false)
    };

    // TODO: Signal handle to cancel all ongoing downloads
    // Client is cloned throughout the codebase because it uses Arc
    // internally. Cloning does not allocate and is the intended way
    // to reuse Client.
    let client = ClientBuilder::new()
        .gzip(true)
        .connect_timeout(Duration::from_secs(3))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:144.0) Gecko/20100101 Firefox/144.0")
        .build()?;

    let mut selenium = None;
    let resources_ = resources;
    let mut resources = Vec::new();
    for res in resources_ {
        resources.push(match res {
            ParsedResource::Pixiv(p) => {
                Resource::Pixiv(p.to_pixiv(client.clone()))
            }
            ParsedResource::Twitter(t) => {
                if selenium.is_none() {
                    selenium = Some(KillOnDropProcess(Command::new("./msedgedriver.exe")
                        .arg("--port=4444")
                        .arg("--silent")
                        .arg("--headless")
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .context("Selenium error")?));
                }
                
                Resource::Twitter(t.to_twitter(client.clone()))
            }
            ParsedResource::Unknown(u) => {
                Resource::Unknown(u)
            }
        });
    }
    
    let mut failed_resources  = Vec::new();
    let (sender, mut receiver) = mpsc::channel(32);
    
    for (i, res) in resources.into_iter().enumerate() {
        // Do we need this delay (429 error)?
        let delay = Duration::from_millis(i as u64 * 500);
        let sender = sender.clone();
        tokio::spawn(async move {
            sleep(delay).await;
            match res {
                Resource::Pixiv(mut pixiv) => {
                    match pixiv.download().await {
                        Err(e) => {
                            let context = format!("[Pixiv ({})] Failed", pixiv.id);
                            println!("{}", format!("{:#}", anyhow::Error::from(e).context(context)).red());
                            sender.send(pixiv.origin).await.unwrap();
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
                            sender.send(pixiv.origin).await.unwrap();
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
                    println!("[Unknown ({})] Skipped", unknown.origin);
                    sender.send(unknown.origin).await.unwrap();
                }
                Resource::Twitter(twitter) => {
                    match twitter.download().await {  
                        Ok(None) => {
                            println!("Twitter success");
                        }
                        Ok(Some(failed_subresources)) => {
                            let sub_id_sequence = failed_subresources.into_iter()
                                .map(|v| v.to_string())
                                .collect::<Box<[String]>>()
                                .join(", ");

                            println!("Twitter partly failed ({sub_id_sequence})");
                            sender.send(twitter.origin).await.unwrap();
                        }
                        Err(err) => {
                            println!("Twitter failed {err:?}");
                            sender.send(twitter.origin).await.unwrap();
                        }
                    }
                }
            }
        });
    }

    drop(sender);

    while let Some(resource_origin) = receiver.recv().await {
        failed_resources.push(resource_origin);
    }

    if failed_resources.is_empty() {
        println!("{}", "All resources have been successfully downloaded!".green());
        let _ = File::create(&input_file_path)?;
    } else if is_interactive {
        println!("{}", format!("Some resources are failed to download or skipped which remains in {:?}.", input_file_path.as_ref()).yellow());

        let mut input_file = BufWriter::new(File::create(&input_file_path)?);
        input_file.write_all(failed_resources.join("\n").as_bytes())?;

        // Manual flush so it's not blocked by stdin
        // This could probably be solved with async IO by using task?
        input_file.flush()?;
    } else {
        let sequence = failed_resources.join("\n");
        println!("{}", format!("The following resources failed to download:\n{sequence}").yellow());
    }

    if is_interactive {
        println!("Press enter to terminate the program.");
        let mut input = String::new();
        stdin().read_line(&mut input)?;
    }

    Ok(())
}