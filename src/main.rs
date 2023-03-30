use chrono::Utc;
use clap::{arg, App, Arg};
use error_chain::error_chain;
use fancy_regex::Regex;
use glob::{glob_with, MatchOptions};
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Debug)]
pub struct StdFloatMathUsedError;
impl std::fmt::Display for StdFloatMathUsedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Std float math used in this crate.")
    }
}
impl std::error::Error for StdFloatMathUsedError {}

error_chain! {
    foreign_links {
        Glob(glob::GlobError);
        Pattern(glob::PatternError);
        Io(std::io::Error);
        Regex(regex::Error);
        Utf8(std::string::FromUtf8Error);
        StdFloatMath(StdFloatMathUsedError);
    }
}

#[derive(Clone, Copy, Debug)]
enum EmitType {
    Mir,
    Asm,
}

#[derive(Clone, Debug)]
struct CmdLineArgs {
    pub tested_crate_path: PathBuf,
    pub tested_package_name: String,
    pub tested_features: Vec<String>,
}

const BUILD_TARGET: &str = "x86_64-pc-windows-msvc";

fn clear_temp_dir(build_dir: &Path) {
    if build_dir.exists() {
        std::fs::remove_dir_all(build_dir)
            .unwrap_or_else(|_| panic!("fail to remove {}", build_dir.display()));
    }
}

fn gen_compiled_file(
    this_crate_dir: &Path,
    tested_crate_dir: &Path,
    tested_features: &Vec<String>,
    build_dir: &Path,
    emit_type: EmitType,
) {
    assert!(this_crate_dir.is_absolute());
    assert!(tested_crate_dir.is_absolute());

    clear_temp_dir(build_dir);
    std::env::set_current_dir(tested_crate_dir)
        .unwrap_or_else(|_| panic!("fail to set current dir to {}", tested_crate_dir.display()));

    let mut cmd = Command::new("cargo");
    cmd.arg("rustc");
    cmd.arg("--no-default-features");
    for feature in tested_features {
        cmd.arg("--features");
        cmd.arg(feature);
    }
    cmd.arg("--target-dir").arg(build_dir);
    cmd.arg("--target").arg(BUILD_TARGET);
    cmd.arg("--").arg("--emit").arg(match emit_type {
        EmitType::Asm => "asm",
        EmitType::Mir => "mir",
    });
    println!("Rustc emit command: {cmd:?}");

    cmd.stdout(Stdio::piped()).spawn().unwrap().wait().unwrap();

    std::env::set_current_dir(this_crate_dir)
        .unwrap_or_else(|_| panic!("fail to set current dir to {}", this_crate_dir.display()));
}

fn compiled_file(
    this_crate_dir: &Path,
    emit_type: EmitType,
    build_dir: &Path,
    tested_crate_name: &str,
) -> Result<PathBuf> {
    assert!(this_crate_dir.is_absolute());

    let emit_file_pat = this_crate_dir
        .join(build_dir)
        .join(BUILD_TARGET)
        .join("debug")
        .join("deps")
        .join(
            String::from(tested_crate_name)
                + "*."
                + match emit_type {
                    EmitType::Asm => "s",
                    EmitType::Mir => "mir",
                },
        );

    let mut files: Vec<PathBuf> = Vec::new();
    for entry in glob_with(
        emit_file_pat.to_str().unwrap(),
        MatchOptions {
            case_sensitive: true,
            ..Default::default()
        },
    )? {
        files.push(entry?);
    }
    if files.len() != 1 {
        if files.is_empty() {
            println!("emit file {} not found", emit_file_pat.display());
        } else {
            println!("multiple emit file {} found", emit_file_pat.display());
        }
        clear_temp_dir(build_dir);
        assert!(files.len() == 1);
    }

    Ok(files[0].clone())
}

fn std_math_used(std_math_patterns: Regex, tested_file: &Path) -> Result<bool> {
    let content = BufReader::new(File::open(tested_file)?);
    let mut std_math_used = false;
    for line in content.lines() {
        let line_content = line?;
        if std_math_patterns.captures(&line_content).unwrap().is_some() {
            if !std_math_used {
                println!("std math usage found in: {}", tested_file.display());
            }
            std_math_used = true;
            println!("std math found: {line_content}");
        };
    }
    Ok(std_math_used)
}

fn test_asm_or_mir(
    this_crate_dir: &Path,
    tested_crate_dir: &Path,
    test_crate_name: &str,
    tested_features: &Vec<String>,
    emit_type: EmitType,
    std_math_patterns: Regex,
    build_dir: &Path,
) -> Result<bool> {
    gen_compiled_file(
        this_crate_dir,
        tested_crate_dir,
        tested_features,
        build_dir,
        emit_type,
    );
    let compiled_file = compiled_file(this_crate_dir, emit_type, build_dir, test_crate_name)?;
    let res = std_math_used(std_math_patterns, &compiled_file);
    clear_temp_dir(build_dir);
    res
}

fn parse_args(current_dir: &Path) -> CmdLineArgs {
    let matches = App::new("NostdFloatMathMonitorApp")
        .version("0.0.2")
        .author("liuxiaonan")
        .about("Detect if std float math function is used in your crate")
        .arg(arg!(-p --path <PATH> "Path of crate you want to test"))
        .arg(
            Arg::with_name("features")
                .short('f')
                .long("features")
                .action(clap::ArgAction::Set)
                .help("Features you want to test"),
        )
        .get_matches();

    let tested_crate_path = Path::new(matches.value_of("path").unwrap());
    let tested_crate_absolute_path = if !Path::new(tested_crate_path).is_absolute() {
        current_dir
    } else {
        Path::new("")
    };
    let tested_crate_absolute_path = tested_crate_absolute_path.join(Path::new(tested_crate_path));
    assert!(
        tested_crate_absolute_path.exists(),
        "path not exists: {}",
        tested_crate_absolute_path.display()
    );

    let tested_package =
        cargo_toml::Manifest::from_path(tested_crate_absolute_path.join("Cargo.toml"))
            .unwrap()
            .package()
            .name
            .clone();

    let features = matches
        .get_many::<String>("features")
        .unwrap_or_default()
        .cloned()
        .collect();

    CmdLineArgs {
        tested_crate_path: tested_crate_absolute_path,
        tested_package_name: tested_package,
        tested_features: features,
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn main() -> Result<()> {
    use error_chain::{ChainedError, State};

    let current_dir = std::env::current_dir()?;
    let args = parse_args(&current_dir);
    let tested_features = args.tested_features;

    let mir_pattern: Regex = Regex::new(
            r"std::(f32|f64)::<impl \1>::(abs|abs_sub|acos|acosh|asin|asinh|atan|atan2|atanh|cbrt|ceil|copysign|cos|cosh|div_euclid|exp|exp2|exp_m1|floor|fract|hypot|ln|ln_1p|log|log10|log2|mul_add|powf|powi|rem_euclid|round|signum|sin|sin_cos|sinh|sqrt|tan|tanh|trunc)"
        )
        .unwrap();
    let asm_pattern: Regex = Regex::new(
            r"std::(f32|f64)::impl\$[0-9]+::(abs|abs_sub|acos|acosh|asin|asinh|atan|atan2|atanh|cbrt|ceil|copysign|cos|cosh|div_euclid|exp|exp2|exp_m1|floor|fract|hypot|ln|ln_1p|log|log10|log2|mul_add|powf|powi|rem_euclid|round|signum|sin|sin_cos|sinh|sqrt|tan|tanh|trunc)"
        )
        .unwrap();
    let build_dir = current_dir.join(
        String::from("temp_build_dir")
            + "_"
            + &args.tested_package_name
            + "_"
            + &format!("{}", Utc::now()).replace([' ', ':', '.'], "-"),
    );

    let std_math_used_in_mir = test_asm_or_mir(
        &current_dir,
        &args.tested_crate_path,
        &args.tested_package_name,
        &tested_features,
        EmitType::Mir,
        mir_pattern,
        &build_dir,
    )
    .unwrap();

    let std_math_used_in_asm = test_asm_or_mir(
        &current_dir,
        &args.tested_crate_path,
        &args.tested_package_name,
        &tested_features,
        EmitType::Asm,
        asm_pattern,
        &build_dir,
    )
    .unwrap();

    if std_math_used_in_mir {
        println!("std math found in mir, non deterministic");
    } else {
        println!("Ok, std math not found in mir");
    }

    if std_math_used_in_asm {
        println!("std math found in asm, non deterministic");
    } else {
        println!("Ok, std math not found in asm");
    }

    if std_math_used_in_asm || std_math_used_in_mir {
        Err(Error::new(
            ErrorKind::StdFloatMath(StdFloatMathUsedError),
            State::default(),
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn main() -> Result<()> {
    println!("Only windows and macos is supported");

    Err(Error::new(
        ErrorKind::StdFloatMath(StdFloatMathUsedError),
        State::default(),
    ))
}
