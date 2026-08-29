//! The magehat command. Argument parsing by hand: a handful of subcommands
//! and three flags do not justify a dependency.

use crate::errors::{MageError, Result};
use std::path::{Path, PathBuf};

/// The manual is the documentation: `magehat -h` prints it, and the release
/// package ships the same text as README.md. Embedding it means an executable
/// on its own still explains itself, with nothing else to look up.
const MANUAL: &str = include_str!("../MANUAL.md");

fn manual() -> String {
    MANUAL.replace("{{VERSION}}", env!("CARGO_PKG_VERSION"))
}

/// A bare `magehat` prints the command list, lifted from the manual's own
/// Commands section so the short help and the reference cannot disagree.
pub fn usage() -> String {
    let manual = manual();
    let commands = manual
        .split_once("\n## Commands\n")
        .map(|(_, rest)| rest)
        .unwrap_or_default()
        .lines()
        .skip_while(|l| l.trim().is_empty())
        .take_while(|l| l.starts_with("    ") || l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "magehat: a tiny, deterministic compiler for plain HTML sites\n\n\
         Usage: magehat <command> [options]\n\n\
         Commands:\n{}\n\n\
         Run `magehat -h` for the manual: install, syntax, everything.\n",
        commands.trim_end()
    )
}

struct Args {
    command: String,
    positional: Vec<String>,
    json: bool,
    port: u16,
    lang: Option<String>,
}

fn parse_args(argv: &[String]) -> Result<Args> {
    let mut args = Args { command: String::new(), positional: Vec::new(), json: false, port: 8080, lang: None };
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => args.json = true,
            "--port" => {
                let v = it.next().ok_or_else(|| MageError::new("--port needs a number"))?;
                args.port = v.parse().map_err(|_| MageError::new(format!("--port needs a number, got {v:?}")))?;
            }
            "--lang" => {
                args.lang = Some(it.next().ok_or_else(|| MageError::new("--lang needs a language code"))?.clone());
            }
            "-h" | "--help" | "help" => args.command = "help".into(),
            "-V" | "--version" | "version" => args.command = "version".into(),
            s if s.starts_with('-') => return Err(MageError::new(format!("unknown option {s}")).fix("run `magehat` for the commands, `magehat -h` for the manual")),
            s if args.command.is_empty() => args.command = s.to_string(),
            s => args.positional.push(s.to_string()),
        }
    }
    Ok(args)
}

/// Find site.toml in the current folder or one of its parents.
fn site_root() -> Result<PathBuf> {
    let here = std::env::current_dir()?;
    let mut dir: Option<&Path> = Some(&here);
    while let Some(d) = dir {
        if d.join("site.toml").is_file() {
            return Ok(d.to_path_buf());
        }
        dir = d.parent();
    }
    Err(MageError::new("no site.toml found here or in a parent folder")
        .fix("create site.toml and src/ as shown under \"A site from nothing\" in `magehat -h`, run `magehat init` for a sample site, or cd into an existing one"))
}

pub fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(&argv) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    };
    std::process::exit(code);
}

fn run(argv: &[String]) -> Result<i32> {
    let args = parse_args(argv)?;
    match args.command.as_str() {
        "" => {
            print!("{}", usage());
            Ok(0)
        }
        "help" => {
            print!("{}", manual());
            Ok(0)
        }
        "version" => {
            println!("magehat {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        "init" => {
            let dir = args.positional.first().map(String::as_str).unwrap_or(".");
            let target = PathBuf::from(dir);
            let written = crate::init::init_site(&target)?;
            println!("Created a MageHat site in {}", target.canonicalize().unwrap_or(target.clone()).display());
            for rel in &written {
                println!("  {rel}");
            }
            let cd = if dir == "." { String::new() } else { format!("cd {dir} && ") };
            println!("\nRun `magehat -h` for the manual, then: {cd}magehat check");
            Ok(0)
        }
        "new" => {
            let root = site_root()?;
            let kind = args.positional.first().map(String::as_str).unwrap_or("");
            let rest = args.positional.get(1..).unwrap_or(&[]);
            println!("{}", crate::new::new(&root, kind, rest, args.lang.as_deref())?);
            Ok(0)
        }
        "add" => match args.positional.first() {
            None => {
                println!("{}", crate::library::listing());
                Ok(0)
            }
            Some(name) => {
                let root = site_root()?;
                println!("{}", crate::library::add(&root, name)?);
                Ok(0)
            }
        },
        "build" => {
            let root = site_root()?;
            let result = crate::build::build_site(&root)?;
            if result.ok() {
                crate::build::write_outputs(&result, &result.cfg.dist())?;
            }
            if args.json {
                println!("{}", serde_json::to_string_pretty(&crate::check::report_json(&result)).unwrap());
            } else if result.ok() {
                println!("Built {} pages into {}", result.pages.len(), result.cfg.dist().display());
                for w in &result.warnings {
                    println!("warning: {w}");
                    if let Some(fix) = &w.fix {
                        println!("  fix: {fix}");
                    }
                }
                for n in &result.notes {
                    println!("note: {n}");
                }
            } else {
                eprintln!("{}", crate::check::format_report(&result));
            }
            Ok(if result.ok() { 0 } else { 1 })
        }
        "check" => {
            let root = site_root()?;
            let result = crate::check::run_check(&root)?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&crate::check::report_json(&result)).unwrap());
            } else {
                println!("{}", crate::check::format_report(&result));
            }
            Ok(if result.ok() { 0 } else { 1 })
        }
        "dev" => {
            let root = site_root()?;
            crate::dev::serve(&root, args.port)?;
            Ok(0)
        }
        "inspect" => {
            let root = site_root()?;
            println!("{}", serde_json::to_string_pretty(&crate::inspect::inspect_site(&root)?).unwrap());
            Ok(0)
        }
        "clean" => {
            let root = site_root()?;
            let mut removed = Vec::new();
            for dir in [root.join("dist"), root.join(".magehat")] {
                if dir.is_dir() {
                    std::fs::remove_dir_all(&dir)?;
                    removed.push(dir.display().to_string());
                }
            }
            if removed.is_empty() {
                println!("Nothing to clean");
            } else {
                println!("Removed {}", removed.join(" and "));
            }
            Ok(0)
        }
        other => Err(MageError::new(format!("unknown command {other:?}")).fix("run `magehat` for the commands, `magehat -h` for the manual")),
    }
}
