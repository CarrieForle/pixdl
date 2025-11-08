use reqwest::Client;
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
pub fn read_input_file<P: AsRef<Path>>(file_path: P, client: Client) -> anyhow::Result<Resources> {
    let file = GeneralOpen.open(file_path)?;
    let reader = io::BufReader::new(file);
    let mut resources = Resources::default();

    for origin in reader.lines() {
        let origin = origin?;
        if origin.trim().is_empty() {
            continue;
        }

        resources.push(Resource::parse(client.clone(), &origin)?);
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

pub async fn run<P: AsRef<Path>>(client: Client, input_file_path: P, arg_resources: Resources) -> anyhow::Result<()> {
    let (resources, is_interactive) = if arg_resources.is_empty() {
        let res = read_input_file(&input_file_path, client.clone())?;

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
                            let origin = pixiv.origin;
                            sender.send(origin).await.unwrap();
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
                            let origin = pixiv.origin;
                            sender.send(origin).await.unwrap();
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
                    let origin = unknown.origin;
                    println!("[Unknown ({})] Skipped", origin);
                    sender.send(origin).await.unwrap();
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
    } else {
        let filename = input_file_path.as_ref().file_name().unwrap();
        println!("{}", format!("Some resources are failed to download or skipped which remains in {:?}.", filename).yellow());

        let mut input_file = BufWriter::new(File::create(&input_file_path)?);
        input_file.write_all(failed_resources.join("\n").as_bytes())?;

        // Manual flush so it's not blocked by stdin
        // This could probably be solved with async IO by using task?
        input_file.flush()?;
    }

    if is_interactive {
        println!("Press enter to terminate the program.");
        let mut input = String::new();
        stdin().read_line(&mut input)?;
    }

    Ok(())
}