use clap::{Parser, command};
use crate::resource::{ParsedResources, Resource};

fn parser(arg: &str) -> anyhow::Result<ParsedResources> {
    if arg.is_empty() {
        return Ok(ParsedResources::new())
    }
    
    let input_res = arg.split(',');
    let mut resources = ParsedResources::new();
    for res in input_res {
        if res.trim().is_empty() {
            continue;
        }
        
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

const DESCRIPTION: &str = r#"On startup, pixdl will find "write.txt" in the program directory. pixdl will create it if it couldn't find "write.txt". This is where you put resources to download. You may also supply resources as command line argument.

A resource is a URL linked to the things you want to download and optionally a bunch of options specific to that resource.

In "write.txt", each resource is separated by a newline. When supplying argument it's separated by comma. The URL and options of a resource are separated by a whitespace.

RESOURCE OPTIONS:
For a Pixiv or Twitter resource, if there are multiple artworks (or subresources) for a given URL, you can optionally specify any number of either range (<start>..<end>) or index of subresources to only download some of the files. The index starts from 1 and the range are inclusive. Not specifying any will download all artworks.
For example: "https:///www.pixiv.net/artworks/1234 1..2" will download the first and second illustration."#;