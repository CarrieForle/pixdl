use reqwest::Client;
use thirtyfour::{DesiredCapabilities, WebDriver};
use tokio::time::sleep;
use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufWriter, ErrorKind, Write, stdin};
use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc::{self};
use colored::Colorize;
use crate::resource::*;

pub mod resource;
pub mod download;
pub mod global;
pub mod command_line;

// Client is cloned throughout the codebase because it uses Arc
// internally. Cloning does not allocate and is the intended way
// to reuse Client.
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

pub async fn run<P: AsRef<Path>>(client: Client, input_file_path: P, arg_resources: ParsedResources) -> anyhow::Result<()> {
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

    let driver = if resources.iter().any(|res| matches!(res, ParsedResource::Twitter(_))) {
        // requires msedgedriver https://developer.microsoft.com/en-gb/microsoft-edge/tools/webdriver
        // TODO: automation so the users do not 
        // need to install edge, download 
        // the driver, and launch the driver.
        // the port is also different per launch.
        let caps = DesiredCapabilities::edge();
        let driver = WebDriver::new("http://localhost:5579", caps).await?;
        Some(driver)
    } else {
        None
    };

    let resources: Vec<_> = resources.into_iter()
        .map(|res| {
            match res {
                ParsedResource::Pixiv(p) => {
                    Resource::Pixiv(p.to_pixiv(client.clone()))
                }
                ParsedResource::Twitter(t) => {
                    let driver = driver.as_ref().unwrap().clone();
                    Resource::Twitter(t.to_twitter(client.clone(), driver))
                }
                ParsedResource::Unknown(u) => {
                    Resource::Unknown(u)
                }
            }
        })
        .collect();

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
                            println!("Twitter failed {err:#?}");
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