use std::{env, io::stdin};
use anyhow::Result;
use pixdl::global::{Global, global};

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(windows)] {
        // https://github.com/colored-rs/colored/issues/110
        colored::control::set_virtual_terminal(true).unwrap();
    }

    let global = global()?;
    const FILE_NAME: &'static str = "write.txt";

    if parse_cmd_argument()? {
        print_help(&global, FILE_NAME);
        return Ok(()); 
    }

    let file_path = global
        .current_directory()
        .join(FILE_NAME);

    pixdl::run(file_path).await?;
    
    println!("Press enter to terminate the program.");
    let mut input = String::new();
    stdin().read_line(&mut input)?;
    Ok(())
}

fn parse_cmd_argument() -> Result<bool> {
    let args: Vec<_> = env::args().collect();
    Ok(args.len() > 1)
}

fn print_help(global: &Global, file_name: &str) {
    let executable_name = global.executable_name();
    print!(
r#"Usage: {executable_name}

DESCRIPTION
pixdl is a pixiv illustration downloader.

USAGE
On startup, pixdl will find "{file_name}" in the program directory. pixdl will create it if it couldn't find "{file_name}". This is where you put resources to download.

A resource is a URL linked to the things you want to download and optionally a bunch of options specific to that resource. There is only one kind of resource: pixiv. In the future I might support more.

In "{file_name}", each resource is separated by a newline. Each line contains a URL and optionally some options. The URL and each option are separated by a whitespace.

In a pixiv resource, the URL should looks like "https://www.pixiv.net/artworks/<illust_id>". If there are multiple artworks for a given URL, you can optionally specify either a range (<start>..<end>) or any number of index of illustration to only download some of the files. The index starts from 1 and the range are inclusive. Not specifying any will download all artworks.

For example: "https://www.pixiv.net/artworks/1234 1..2" will download the first and second illustration.
"#);
}