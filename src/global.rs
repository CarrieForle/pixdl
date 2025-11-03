use anyhow::{anyhow, Result};
use std::path::{PathBuf};
use std::env;

/// Global state that should be precomputed once on main() and pass around the codebase.
/// Fields are private and remain immutable.
#[derive(Debug)]
pub struct Global {
    current_directory: PathBuf,
    executable_name: String,
}

impl Global {
    pub const fn current_directory(&self) -> &PathBuf {
        &self.current_directory
    }

    pub const fn executable_name(&self) -> &String {
        &self.executable_name
    }
}

/// Compute Global.
/// This should only be called once on `main()` and intended to pass around the codebase.
pub fn global() -> Result<Global> {
    let mut current_exe = env::current_exe()?;

    let executable_name = current_exe.file_name()
        .ok_or(anyhow!("Failed to get current exe name."))?
        .to_string_lossy()
        .into_owned();

    let current_directory = if env::var("rust_debug").unwrap_or("0".to_string()) == "0" {
        PathBuf::new()
    } else {
        current_exe.pop();
        current_exe
    };
    
    Ok(Global {
        current_directory,
        executable_name,
    })
}