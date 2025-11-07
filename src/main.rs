use std::time::Duration;
use anyhow::Result;
use pixdl::global::global;
use pixdl::command_line;
use pixdl::resource::Resources;
use reqwest::ClientBuilder;

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(windows)] {
        // https://github.com/colored-rs/colored/issues/110
        colored::control::set_virtual_terminal(true).unwrap();
    }

    let client = ClientBuilder::new()
        .gzip(true)
        .read_timeout(Duration::from_secs(2))
        .connect_timeout(Duration::from_secs(3))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:144.0) Gecko/20100101 Firefox/144.0")
        .build()?;

    let global = global()?;
    const FILE_NAME: &str = "write.txt";
    let file_path = global
        .current_directory()
        .join(FILE_NAME);

    let mut cli = command_line::populate(client.clone(), FILE_NAME);
    let resources: Resources = cli.remove_one("resources").
        unwrap_or_default();

    pixdl::run(client, file_path, resources).await?;

    Ok(())
}