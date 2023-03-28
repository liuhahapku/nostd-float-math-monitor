use error_chain::error_chain;
use fancy_regex::Regex;
use glob::{glob_with, MatchOptions};
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

error_chain! {
    foreign_links {
        Glob(glob::GlobError);
        Pattern(glob::PatternError);
        Io(std::io::Error);
        Regex(regex::Error);
        Utf8(std::string::FromUtf8Error);
    }
}

#[derive(Clone, Copy, Debug)]
enum EmitType {
    MIR,
    ASM,
}

const TEMP_BUILD_DIR: &str = "temp_for_test";
const BUILD_TARGET: &str = "x86_64-pc-windows-msvc";

fn gen_asm_or_mir(
    this_proj_dir: &Path,
    tested_proj_dir: &Path,
    tested_features: &Vec<&str>,
    emit_type: EmitType,
) {
    assert!(this_proj_dir.is_absolute());
    let build_dir = this_proj_dir.join(TEMP_BUILD_DIR);
    if build_dir.exists() {
        std::fs::remove_dir_all(build_dir.clone()).unwrap();
    }

    std::env::set_current_dir(tested_proj_dir).unwrap();

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
        EmitType::ASM => "asm",
        EmitType::MIR => "mir",
    });
    cmd.stdout(Stdio::piped()).spawn().unwrap().wait().unwrap();

    std::env::set_current_dir(this_proj_dir).unwrap();
}

fn asm_or_mir_file(this_proj_dir: &Path, emit_type: EmitType) -> Result<PathBuf> {
    let emit_file_pat = this_proj_dir
        .join(TEMP_BUILD_DIR)
        .join(BUILD_TARGET)
        .join("debug")
        .join("deps")
        .join(
            String::from("glam_motphys-*.")
                + match emit_type {
                    EmitType::ASM => "s",
                    EmitType::MIR => "mir",
                },
        );
    let mut file_vec: Vec<PathBuf> = Vec::new();
    for entry in glob_with(
        emit_file_pat.to_str().unwrap(),
        MatchOptions {
            case_sensitive: true,
            ..Default::default()
        },
    )? {
        file_vec.push(entry?);
    }
    assert!(file_vec.len() == 1);
    Ok(file_vec[0].clone())
}

fn test_std_math_used(std_math_patterns: Regex, tested_file: &Path) -> Result<bool> {
    let content = BufReader::new(File::open(tested_file)?);
    let mut std_math_used = false;
    for line in content.lines() {
        let line_content = line?;
        if !std_math_patterns.captures(&line_content).unwrap().is_none() {
            std_math_used = true;
            println!("std math find: {}", line_content);
        };
    }
    Ok(std_math_used)
}

fn test_asm_or_mir(
    this_proj_dir: &Path,
    tested_proj_dir: &Path,
    tested_features: &Vec<&str>,
    emit_type: EmitType,
    std_math_patterns: Regex,
) -> Result<bool> {
    gen_asm_or_mir(this_proj_dir, tested_proj_dir, tested_features, emit_type);
    let build_res_file = asm_or_mir_file(this_proj_dir, emit_type)?;
    let build_dir = this_proj_dir.join(TEMP_BUILD_DIR);
    println!("detect std math usage in {}", build_res_file.display());
    let res = test_std_math_used(std_math_patterns, &build_res_file);
    if build_dir.exists() {
        std::fs::remove_dir_all(build_dir.clone())?;
    }
    res
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn main() -> Result<()> {
    use clap::App;

    let matches = App::new("NostdFloatMathMonitorApp")
        .version("0.0.1")
        .author("liuxiaonan")
        .about("XXX!")
        .args_from_usage("-p, --path=[FILE] 'Target crate you want to test'")
        .get_matches();

    let tested_project_dir = if let Some(f) = matches.value_of("path") {
        println!("path : {}", f);
        f
    } else {
        panic!()
    };

    let mir_pattern: Regex = Regex::new(
            r"std::(f32|f64)::<impl \1>::(abs|abs_sub|acos|acosh|asin|asinh|atan|atan2|atanh|cbrt|ceil|copysign|cos|cosh|div_euclid|exp|exp2|exp_m1|floor|fract|hypot|ln|ln_1p|log|log10|log2|mul_add|powf|powi|rem_euclid|round|signum|sin|sin_cos|sinh|sqrt|tan|tanh|trunc)"
        )
        .unwrap();

    let asm_pattern: Regex = Regex::new(
            r"std::(f32|f64)::impl\$[0-9]+::(abs|abs_sub|acos|acosh|asin|asinh|atan|atan2|atanh|cbrt|ceil|copysign|cos|cosh|div_euclid|exp|exp2|exp_m1|floor|fract|hypot|ln|ln_1p|log|log10|log2|mul_add|powf|powi|rem_euclid|round|signum|sin|sin_cos|sinh|sqrt|tan|tanh|trunc)"
        )
        .unwrap();

    let this_proj_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tested_proj_dir = Path::new(tested_project_dir);
    let tested_features = vec!["std"];

    let std_math_used_in_mir = test_asm_or_mir(
        &this_proj_dir,
        tested_proj_dir,
        &tested_features,
        EmitType::MIR,
        mir_pattern,
    );

    let std_math_used_in_asm = test_asm_or_mir(
        &this_proj_dir,
        tested_proj_dir,
        &tested_features,
        EmitType::ASM,
        asm_pattern,
    );

    if std_math_used_in_mir.unwrap() {
        println!("std math used in mir, non deterministic");
    } else {
        println!("Ok, std math not used in mir");
    }

    if std_math_used_in_asm.unwrap() {
        println!("std math used in asm, non deterministic");
    } else {
        println!("Ok, std math not used in asm");
    }

    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn main() -> Result<()> {
    println!("Only windows and macos is supported");

    Ok(())
}
