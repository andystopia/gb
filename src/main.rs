#![allow(dead_code)]

use std::{error::Error, process::Command};

use clap::Parser;
use colored::Colorize;
use toml_edit::Document;

#[derive(Debug)]

pub enum Level {
    Fatal,
    Error,
    Warning,
    Info,
}

#[derive(Debug)]
pub struct GbError {
    message: String,
    level: Level,
    source: Option<Box<dyn Error + Send + Sync + 'static>>,
}

trait Check<T> {
    fn fatal(self, message: impl Into<String>) -> Result<T, GbError>;
}

impl<T, E: std::error::Error + Send + Sync + 'static> Check<T> for Result<T, E> {
    fn fatal(self, message: impl Into<String>) -> Result<T, GbError> {
        self.map_err(|e| GbError {
            message: message.into(),
            level: Level::Fatal,
            source: Some(Box::new(e)),
        })
    }
}

impl<T> Check<T> for Option<T> {
    fn fatal(self, message: impl Into<String>) -> Result<T, GbError> {
        self.ok_or_else(|| GbError {
            message: message.into(),
            level: Level::Fatal,
            source: None,
        })
    }
}

impl std::fmt::Display for GbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "{} {}: {}. Aborting.",
            "[gb-error]".red().bold(),
            "[build]".blue().bold(),
            self.message
        ))
    }
}
impl std::error::Error for GbError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.source {
            Some(source) => Some(source.as_ref()),
            None => None,
        }
    }

    fn description(&self) -> &str {
        "description() is deprecated; use Display"
    }

    fn cause(&self) -> Option<&dyn std::error::Error> {
        self.source()
    }
}

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

    if let Err(e) = validate(&commands) {
        eprintln!("{}", e);
        std::process::exit(1);
    }

    Ok(())
}

fn validate(commands: &Commands) -> Result<(), GbError> {
    // let pwd = current_dir().error("cannot get the current directory")?;
    let manifest = std::fs::read_to_string("gb.toml")
        .fatal("manifest file `gb.toml` not found in the current directory")?;
    let doc = manifest
        .parse::<Document>()
        .fatal("failed to parse manifest file")?;
    let default_target = doc
        .as_item()
        .get("default")
        .and_then(|default| default.get("target"))
        .and_then(|default_target| default_target.as_str());
    let target = commands
        .target()
        .or(default_target)
        .fatal("No target was passed and no default target was set. Aborting.")?;
    let target_info = doc
        .as_item()
        .get("target")
        .fatal("there are no provided targets; please provide them")?
        .get(target)
        .fatal(format!(
            "Attempted to run target `{target}` but it was not found in gb.toml"
        ))?;
    let files = target_info
        .get("files")
        .fatal(format!(
            "a files key is required for every target but it was not supplied for {target}"
        ))?
        .as_array()
        .fatal("the files list must be an array")?
        .into_iter()
        .map(|f| f.as_str())
        .collect::<Option<Vec<&str>>>()
        .fatal("all the files in the files list, must be listed by their path as a string")?;

    let file_to_execute = target_info.get("execute").and_then(|file| file.as_str());

    let missing_files = files
        .iter()
        .filter(|f| !std::path::Path::new(f).exists())
        .collect::<Vec<_>>();
    if missing_files.len() > 0 {
        eprintln!("The following files are listed in the toml target, but were not found");
        for (pos, file) in missing_files.iter().enumerate() {
            eprintln!("  {}. {file}", pos + 1)
        }
        Err(GbError {
            message: "There were missing files.".to_owned(),
            level: Level::Fatal,
            source: None,
        })?;
    }

    match commands {
        Commands::Run { target: _, vcd } => {
            let file_to_exec = file_to_execute.fatal("must have a file chosen to execute")?;
            let child = Command::new("ghdl")
                .arg("-a")
                .args(files)
                .spawn()
                .fatal("couldn't spawn ghdl subprocess")?;
            await_vhdl_process(
                child,
                "couldn't await ghdl analyze subprocess, is ghdl installed?",
            )?;

            let child = Command::new("ghdl")
                .arg("-e")
                .arg(
                    std::path::Path::new(file_to_exec)
                        .file_stem()
                        .fatal("could not get base filename")?,
                )
                .spawn()
                .fatal("couldn't spawn ghdl elaborate subprocess, is ghdl installed?")?;

            let child = Command::new("ghdl")
                .arg("-r")
                .arg(
                    std::path::Path::new(file_to_exec)
                        .file_stem()
                        .fatal("could not get base filename")?,
                )
                .args(match vcd {
                    Some(vcd) => [format!("--vcd={}", vcd.to_string_lossy())].to_vec(),
                    None => vec![],
                })
                .spawn()
                .fatal("couldn't spawn ghdl run subprocess, is ghdl installed?")?;

            await_vhdl_process(child, "couldn't await ghdl run subprocess, is ghdl installed correctly, and do you have run permissions?")?;
        }
        Commands::Compile { target: _ } => {
            let child = Command::new("ghdl")
                .arg("-a")
                .args(files)
                .spawn()
                .fatal("couldn't spawn ghdl subprocess")?;
            await_vhdl_process(
                child,
                "couldn't await ghdl analyze subprocess, is ghdl installed?",
            )?;
        }
    }

    Ok(())
}

fn await_vhdl_process(mut child: std::process::Child, message: &str) -> Result<(), GbError> {
    let waiting = child.wait().fatal(message)?;
    Ok(if !waiting.success() {
        Err(GbError {
            message: "GHDL didn't compile successfully.".to_owned(),
            level: Level::Fatal,
            source: None,
        })?;
    })
}
