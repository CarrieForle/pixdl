use clap::{Parser, command};
use crate::resource::{ParsedResources, Resource};

fn parser(arg: &str) -> anyhow::Result<ParsedResources> {
    if arg.is_empty() {
        return Ok(ParsedResources::new())
    }
    
    let input_res = arg.split(',');
    let mut resources = ParsedResources::new();
    for res in input_res {
        println!("{res}");
        match Resource::parse(res)  {
            Ok(res) => resources.push(res),
            Err(err) => println!("{err:?}")
        }
    }

    Ok(resources)
}

#[derive(Parser)]
#[command(version, about, after_help = DESCRIPTION)]
#[command(about = "pixdl is a pixiv illustration downloader.")]
pub struct Cli {
    #[arg(value_parser = parser)]
    #[arg(required = false)]
    #[arg(default_value = "")]
    #[arg(hide_default_value = true)]
    #[arg(help = "The resources to download")]
    pub resources: ParsedResources,

    #[arg(long)]
    #[arg(default_value_t = false)]
    #[arg(help = "Start login process on startup")]
    pub force_login: bool,
}

const DESCRIPTION: &str = r#"On startup, pixdl will find "write.txt" in the program directory. pixdl will create it if it couldn't find "write.txt". This is where you put resources to download.

A resource is a URL linked to the things you want to download and optionally a bunch of options specific to that resource. There is only one kind of resource: pixiv. In the future I might support more.

In "write.txt", each resource is separated by a newline. Each line contains a URL and optionally some options. The URL and each option are separated by a whitespace.
In a pixiv resource, the URL should looks like "https:///www.pixiv.net/artworks/<illust_id>". If there are multiple artworks for a given URL, you can optionally specify either a range (<start>..<end>) or any number of index of illustration to only download some of the files. The index starts from 1 and the range are inclusive. Not specifying any will download all artworks.c
For example: "https:///www.pixiv.net/artworks/1234 1..2" will download the first and second illustration."#;