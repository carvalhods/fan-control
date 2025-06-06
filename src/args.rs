use std::path::PathBuf;

use clap::{Parser, ValueHint};

fn version() -> String {
    format!(
        "{}-{}",
        env!("FAN_CONTROL_VERSION"),
        env!("FAN_CONTROL_COMMIT")
    )
}

#[derive(Parser, Debug, Default)]
#[clap(author = "wiiznokes", version = version(), about = "fan control app", long_about = None)]
pub struct Args {
    #[arg(
        short = 'p',
        long = "path",
        value_hint = ValueHint::DirPath,
        value_names = ["PATH"],
        help = "Config directory"
    )]
    pub config_dir_path: Option<PathBuf>,

    #[arg(
        short = 'c',
        long = "config",
        value_hint = ValueHint::FilePath,
        value_names = ["PATH"],
        help = "Config file to use, within config directory"
    )]
    pub config_name: Option<String>,

    #[cfg(feature = "ui")]
    #[arg(
        long = "cli",
        default_value_t = false,
        help = "Do not use the graphical interface"
    )]
    pub cli: bool,

    #[arg(
        long = "debug",
        default_value_t = false,
        help = "Access debug level logs"
    )]
    pub debug: bool,

    #[arg(
        long = "info",
        default_value_t = false,
        help = "Access info level logs"
    )]
    pub info: bool,

    #[arg(
        long = "log",
        value_hint = ValueHint::FilePath,
        value_names = ["PATH"],
        help = "Puts logs to a specific file. Usefull on Windows because logs cannot be displayed due to limitations"
    )]
    pub log_file: Option<PathBuf>,

    #[arg(
        long = "write-hardware",
        default_value_t = false,
        help = "Write hardware file in the config dir"
    )]
    pub serialize_hardware: bool,
}
