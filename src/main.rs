use std::{default, env::current_dir, process::Stdio};

use clap::Parser;
use color_eyre::eyre::{bail, ContextCompat};
use toml_edit::Document;

#[derive(Debug, Clone, Parser)]

/// A TOML based build tool using GHDL + VHDL
pub enum Commands {
    /// fully analyze, elaborate, and run
    Run {
        /// choose a target to run with vhdl
        target: Option<String>,
        /// output a vcd file
        vcd: Option<std::path::PathBuf>,
    },

    /// compile a configuration (useful for errors!), only analyzes
    #[clap(alias = "analyze")]
    Compile {
        /// compile a specific target
        target: Option<String>,
    },
}

impl Commands {
    pub fn target(&self) -> Option<&str> {
        match self {
            Commands::Run { target, vcd: _ } => target.as_ref().map(|i| i.as_ref()),
            Commands::Compile { target } => target.as_ref().map(|i| i.as_ref()),
        }
    }
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let commands = Commands::parse();

    let pwd = current_dir()?;

    // let's make sure the files exist

    let manifest = std::fs::read_to_string("gb.toml")?;

    let doc = manifest.parse::<Document>()?;

    let default_target = doc
        .as_item()
        .get("default")
        .and_then(|default| default.get("target"))
        .and_then(|default_target| default_target.as_str());

    let target = commands
        .target()
        .or(default_target)
        .context("No target was passed and no default target was set. Aborting.")?;

    let target_info = doc
        .as_item()
        .get("target")
        .context("there are no provided targets; please provide them")?
        .get(target)
        .context(format!(
            "Attempted to run target `{target}` but it was not found in gb.toml"
        ))?;

    let files = target_info
        .get("files")
        .context(format!(
            "a files key is required for every target but it was not supplied for {target}"
        ))?
        .as_array()
        .context("the files list must be an array")?
        .into_iter()
        .map(|f| f.as_str())
        .collect::<Option<Vec<&str>>>()
        .context("all the files in the files list, must be listed by their path as a string")?;

    let file_to_execute = target_info.get("execute").and_then(|file| file.as_str());

    let missing_files = files
        .iter()
        .filter(|f| !std::path::Path::new(f).exists())
        .collect::<Vec<_>>();

    if missing_files.len() > 0 {
        println!("The following files could not be found: ");
        for (pos, file) in missing_files.iter().enumerate() {
            println!("  {}. {file}", pos + 1)
        }
        bail!("There were files which were missing from the compilation! Aborting.");
    }

    match commands {
        Commands::Run { target: _, vcd } => todo!(),
        Commands::Compile { target: _ } => {
            let mut child = std::process::Command::new("ghdl")
                .arg("-a")
                .args(files)
                .spawn()?;
            let waiting = child.wait()?;

            if !waiting.success() {
                bail!("Compilation with GHDL unsuccessful! Bail!");
            }
        }
    }

    dbg!(file_to_execute);

    Ok(())
}
