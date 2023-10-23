#![allow(dead_code)]

use std::{
    borrow::Cow,
    error::Error,
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    process::Command,
    str::FromStr,
};

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
        #[arg(long)]
        vcd: Option<std::path::PathBuf>,
    },

    /// analyze *and* elaborate a solution
    #[clap(alias = "build")]
    Compile { target: Option<String> },

    /// analyzes a configuration (useful for errors!), only analyzes
    Analyze {
        /// compile a specific target
        target: Option<String>,
    },

    /// Use a waveform viewer, default.vcd-viewer to specify.
    /// will do a run and then view the wave, in a detached
    /// process
    Wave {
        target: Option<String>,
        #[arg(long)]
        vcd: Option<std::path::PathBuf>,
    },

    /// Initilize a ghdl project with gb as the build system.
    Init,
}

impl Commands {
    pub fn target(&self) -> Option<&str> {
        match self {
            Commands::Run { target, vcd: _ } => target.as_ref().map(|i| i.as_ref()),
            Commands::Compile { target } => target.as_ref().map(|i| i.as_ref()),
            Commands::Analyze { target } => target.as_ref().map(|i| i.as_ref()),
            Commands::Init => None,
            Commands::Wave { target, vcd: _ } => target.as_ref().map(|i| i.as_ref()),
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
    if let Commands::Init = commands {
        init()?;
        return Ok(());
    }

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
    let default_vcd_viewer = doc
        .as_item()
        .get("default")
        .and_then(|default| default.get("vcd-viewer"))
        .and_then(|default_target| default_target.as_str());
    let target = commands
        .target()
        .or(default_target)
        .fatal("No target was passed and no default target was set")?;
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

    let vcd_output_name = target_info
        .get("vcd-name")
        .and_then(|i| i.as_str())
        .map(std::path::PathBuf::from);

    match commands {
        Commands::Compile { target: _ } => {
            analyze_vhdl(files, " [1/2] ")?;

            elaborate_vhdl_solution(file_to_execute, " [2/2] ")?;
        }
        Commands::Run { target: _, vcd } => {
            analyze_vhdl(files, " [1/3] ")?;

            let file_to_exec = elaborate_vhdl_solution(file_to_execute, " [2/3] ")?;

            execute_vhdl_solution(file_to_exec, vcd.clone().or(vcd_output_name), " [3/3]")?;
        }
        Commands::Analyze { target: _ } => {
            analyze_vhdl(files, " [1/1] ")?;
        }
        Commands::Wave { target: _, vcd } => {
            let vcd = vcd.clone().or(vcd_output_name);
            analyze_vhdl(files, " [1/3] ")?;

            let file_to_exec = elaborate_vhdl_solution(file_to_execute, " [2/3] ")?;

            execute_vhdl_solution(file_to_exec, vcd.clone(), " [3/3]")?;

            launch_vcd_viewer(vcd, default_vcd_viewer)?;
        }
        Commands::Init => init()?,
    }

    Ok(())
}

fn launch_vcd_viewer(
    vcd: Option<std::path::PathBuf>,
    default_vcd_viewer: Option<&str>,
) -> Result<(), GbError> {
    if vcd.is_none() {
        Err(GbError {
            message: "vcd-name not set in target for toml. Cannot launch vcd viewer".to_owned(),
            level: Level::Fatal,
            source: None,
        })?;
    }
    if default_vcd_viewer.is_none() {
        Err(GbError {
            message: "top level `default.vcd-viewer` not set in target for toml. Cannot launch vcd viewer"
                .to_owned(),
            level: Level::Fatal,
            source: None,
        })?;
    }
    if default_vcd_viewer != Some("gtkwave") {
        Err(GbError {
            message: "The only supported wave viewer is gtkwave. Please set `default.vcd-viewer = \"gtkwave\"`"
                .to_owned(),
            level: Level::Fatal,
            source: None,
        })?;
    }
    eprintln!("launching waveform viewer");

    Command::new("gtkwave")
        .arg(PathBuf::from("build/root/").join(vcd.unwrap()))
        .spawn()
        .fatal("could not create gtkwave")?
        .wait()
        .fatal("failed to await gtkwave")?;

    Ok(())
}

#[cfg(target_os = "macos")]
fn get_macos_version() -> String {
    use std::process::Stdio;

    let cmd = Command::new("sw_vers")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("could not access macos version");

    let output = cmd
        .wait_with_output()
        .expect("could not access macos version");

    if output.status.success() {
        let str = String::from_utf8_lossy(output.stdout.as_slice());

        str.lines()
            .filter(|line| line.starts_with("ProductVersion:"))
            .next()
            .map(|line| line.trim_start_matches("ProductVersion:").trim())
            .expect("failed to parse macos version")
            .to_owned()
    } else {
        panic!("could not access macos version")
    }
}
fn execute_vhdl_solution(
    file_to_exec: &str,
    vcd: Option<std::path::PathBuf>,
    step: &str,
) -> Result<(), GbError> {
    eprintln!(
        "  {}  {}",
        step.blue().bold(),
        "Executing Solution...".green().bold()
    );
    let child = Command::new("ghdl")
        .arg("-r")
        .current_dir("build/root/")
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
    Ok(())
}

fn elaborate_vhdl_solution<'s>(
    file_to_execute: Option<&'s str>,
    step: &str,
) -> Result<&'s str, GbError> {
    eprintln!(
        "  {}  {}",
        step.blue().bold(),
        "Elaborating Solution...".green().bold()
    );
    let file_to_exec = file_to_execute.fatal("must have a file chosen to execute in order to elaborate. Please set `execute = \"<YOUR_FILE>\" in gb.toml")?;

    let args = if cfg!(target_os = "macos") { 
        vec!["-e".to_owned(), format!("-Wl,-mmacosx-version-min={}", get_macos_version())]
    } else { 
        vec!["-e".to_owned()]
    };
    let child = Command::new("ghdl")
        .args(&args)
        .arg(
            std::path::Path::new(file_to_exec)
                .file_stem()
                .fatal("could not get base filename")?,
        )
        .current_dir("build/root/")
        .spawn()
        .fatal("couldn't spawn ghdl elaborate subprocess, is ghdl installed?")?;
    await_vhdl_process(child, "couldn't await ghdl elaborate subprocess, is ghdl installed correctly, and do you have run permissions?")?;

    eprintln!(
        "  {}  {}",
        step.blue().bold(),
        "Successfully Elaborated.".green().bold()
    );
    Ok(file_to_exec)
}

fn analyze_vhdl(files: Vec<&str>, steps: &str) -> Result<(), GbError> {
    eprintln!(
        "  {}  {}",
        steps.blue().bold(),
        "Analyzing Solution...".green().bold()
    );
    compile_vhd_files(files)?;
    eprintln!(
        "  {}  {}",
        steps.blue().bold(),
        "Successfully Analyzed.".green().bold()
    );
    Ok(())
}

fn compile_vhd_files(files: Vec<&str>) -> Result<(), GbError> {
    let child = Command::new("ghdl")
        .arg("-a")
        .args(&files)
        .spawn()
        .fatal("couldn't spawn ghdl subprocess")?;
    {
        let mut child = child;
        let waiting = child
            .wait()
            .fatal("couldn't await ghdl analyze subprocess, is ghdl installed?")?;
        cleanup_build_dir(files)?;
        Ok(if !waiting.success() {
            Err(GbError {
                message: "GHDL didn't compile successfully.".to_owned(),
                level: Level::Fatal,
                source: None,
            })?;
        })
    }?;

    Ok(())
}

fn cleanup_build_dir(files: Vec<&str>) -> Result<(), GbError> {
    move_work_obj93_to_build_directory()?;
    move_artifacts_to_build_directory(files)?;
    Ok(())
}

fn move_artifacts_to_build_directory(files: Vec<&str>) -> Result<(), GbError> {
    std::fs::create_dir_all("build/root/").fatal("could not create build directory")?;
    for file_str in files {
        let file = std::path::Path::new(file_str);

        let stem = file
            .file_stem()
            .fatal(format!("could not get file stem for {file_str}"))?;
        // TODO: Might need to not add the .o on some systems.

        let path = std::path::PathBuf::from(stem).with_extension("o");

        std::fs::rename(&path, std::path::PathBuf::from("build/root/").join(&path)).fatal(
            format!("could not move generated build artifact `{path:?}` to build dir"),
        )?;
    }
    Ok(())
}

fn move_work_obj93_to_build_directory() -> Result<(), GbError> {
    // this method is actually a little more complicated than you might *initially* think, since
    // we need to "fix-up" some of the file paths inside of the file, so that we can still compile
    // the sources. The goal of gb is to be opinionated and flexible while hiding away the details
    // of the what really makes ghdl tick. We just want our traditional build / run steps, basically.
    // Like for instance in C, most of the time it's build, link, run. But generally we just think of build
    // and run. It's like that.

    let file = std::fs::read_to_string("work-obj93.cf").fatal("could not load work-obj93.cf, which is a necessary compliation artifact to move it to the build dir")?;

    let mut lines = file.lines().map(Cow::Borrowed).collect::<Vec<Cow<str>>>();

    const PREFIX: &str = "file . \"";
    for line in &mut lines {
        if line.starts_with(PREFIX) {
            let mut string: String = line.clone().into_owned();

            string.insert_str(PREFIX.len(), "../../");

            *line = Cow::Owned(string);
        }
    }

    let full = lines.join("\n");

    std::fs::create_dir_all("build/root/")
        .fatal("could not create the build directory, but it is necessary to run ghdl")?;

    std::fs::write("build/root/work-obj93.cf", full)
        .fatal("could not move modified work-obj93.cf, but it is necessary to build ghdl")?;

    std::fs::remove_file("work-obj93.cf").fatal("could not remove work-obj93.cf")?;

    Ok(())
}

fn init() -> Result<(), GbError> {
    let exists = PathBuf::from_str("gb.toml").unwrap().exists();

    if exists {
        eprintln!("already inited!");
        return Ok(());
    }

    let mut ignore = OpenOptions::new()
        .append(true)
        .create(true)
        .open(".gitignore")
        .fatal("could not create .gitignore file")?;

    ignore
        .write_all("/build".as_bytes())
        .fatal("could not write '/build' into .gitignore")?;

    std::fs::write(
        "gb.toml",
        r#"
    default.target = "default-target"
    default.vcd-viewer = "gtkwave"
    
    [target.default-target]
    files = []
    
    # execute = "your-file-to-execute"
    # vcd-name = "your-vcd-name.vcd"
    "#,
    )
    .fatal("could not write sample gb.toml")?;

    std::fs::create_dir_all("src/").fatal("could not create src/ dir")?;

    Ok(())
}
fn create_build_src() -> Result<(), GbError> {
    std::fs::create_dir_all("build/src/")
        .fatal("could not construct directory for build source files")
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
