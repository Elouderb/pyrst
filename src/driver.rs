use std::path::Path;
use std::process::Command;

use crate::diag::{Error, Result};

pub fn check(path: &Path) -> Result<()> {
    let prog = crate::resolver::resolve(path)?;
    for (m, _src) in &prog.modules {
        crate::typeck::check_bodies(m, &prog.ctx)?;
    }
    eprintln!("ok: {} module(s) typecheck", prog.modules.len());
    Ok(())
}

pub fn emit(path: &Path) -> Result<()> {
    let rust = compile_to_rust(path)?;
    print!("{}", rust);
    Ok(())
}

pub fn build(path: &Path) -> Result<()> {
    let rust = compile_to_rust(path)?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("a");
    let rs_path = std::env::temp_dir().join(format!("pyrst-{}.rs", stem));
    let bin_path = std::env::current_dir()?.join(stem);
    std::fs::write(&rs_path, rust)?;

    let status = Command::new("rustc")
        .arg(&rs_path)
        .arg("-o")
        .arg(&bin_path)
        .arg("--edition")
        .arg("2021")
        .status()
        .map_err(|e| Error::Rustc(format!("failed to invoke rustc: {}", e)))?;

    if !status.success() {
        return Err(Error::Rustc(format!("rustc exited with status {}", status)));
    }

    eprintln!("built: {}", bin_path.display());
    Ok(())
}

pub fn fmt(path: &Path) -> Result<()> {
    let source = std::fs::read_to_string(path)?;
    let module = crate::parser::parse(&source)?;
    let formatted = crate::formatter::format(&module);
    std::fs::write(path, formatted)?;
    eprintln!("formatted: {}", path.display());
    Ok(())
}

fn compile_to_rust(path: &Path) -> Result<String> {
    let prog = crate::resolver::resolve(path)?;
    for (m, _src) in &prog.modules {
        crate::typeck::check_bodies(m, &prog.ctx)?;
    }
    crate::codegen::emit_program(&prog.modules, &prog.ctx)
}
