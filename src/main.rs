use anyhow::Result;
use clap::Parser;
use pixdl::global::global;
use pixdl::command_line::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(windows)] {
        // https://github.com/colored-rs/colored/issues/110
        colored::control::set_virtual_terminal(true).unwrap();
    }

    let global = global()?;
    const FILE_NAME: &str = "write.txt";
    let file_path = global
        .current_directory()
        .join(FILE_NAME);

    let cli = Cli::parse();

    pixdl::run(file_path, cli).await?;

    Ok(())
}