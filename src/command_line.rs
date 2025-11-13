use clap::{ArgMatches, arg, command};
use reqwest::Client;
use crate::resource::{Resource, Resources};

fn parser(client: Client, arg: &str) -> anyhow::Result<Resources> {
    let input_res = arg.split(',');
    let mut resources = Resources::new();

    for res in input_res {
        match Resource::parse(client.clone(), res)  {
            Ok(res) => resources.push(res),
            Err(err) => println!("{err:?}")
        }
    }

    Ok(resources)
}

/// This not only parse but also create Resource.
/// We need [Client] because it's part of the required field.
/// 
/// An alternate approach is creating IntermediateResource
/// struct that stores parsed information which can later
/// be populated from [main]. But it's kind of complicated and there
/// is no way to update intermediate and final resource at once.
pub fn populate(client: Client, file_name: &str) -> ArgMatches {
    command!()
        .about("A pixiv illustration downloader.")
        .after_help(description(file_name))
        .arg(arg!([resources] "Resources to download")
            .value_parser(move |s: &str| parser(client.clone(), s))
        )
        .get_matches()
}

fn description(file_name: &str) -> String {
    format!(r#"On startup, pixdl will find "{file_name}" in the program directory. pixdl will create it if it couldn't find "{file_name}". This is where you put resources to download.

A resource is a URL linked to the things you want to download and optionally a bunch of options specific to that resource. There is only one kind of resource: pixiv. In the future I might support more.

In "{file_name}", each resource is separated by a newline. Each line contains a URL and optionally some options. The URL and each option are separated by a whitespace.
In a pixiv resource, the URL should looks like "https:///www.pixiv.net/artworks/<illust_id>". If there are multiple artworks for a given URL, you can optionally specify either a range (<start>..<end>) or any number of index of illustration to only download some of the files. The index starts from 1 and the range are inclusive. Not specifying any will download all artworks.c
For example: "https:///www.pixiv.net/artworks/1234 1..2" will download the first and second illustration."#)
}