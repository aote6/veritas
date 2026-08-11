use std::env;
use std::fs;
use std::process;
use veritas_kernel::assembler::assemble_module;
use veritas_kernel::module::{ModuleImage, ModuleLoader};
use veritas_kernel::kernel::Kernel;
use veritas_kernel::runtime::{Runtime, ExecutionOutcome};
use std::sync::Arc;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: veritas <compile|run|info|inspect|serve> [args...]");
        eprintln!("  compile <in.vasm> <out.vmod>   Assemble a .vasm file");
        eprintln!("  run <file.vmod> [wal_path]     Execute a module (optional persistent WAL)");
        eprintln!("  info <file.vmod>               Show module metadata");
        eprintln!("  inspect [list|object <id>]     Inspect kernel state (needs WAL)");
        eprintln!("  serve <wal_path>               Start JSON-RPC daemon");
        process::exit(1);
    }

    let result = run_command(&args);
    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

fn get_kernel(args: &[String], wal_arg_pos: usize) -> Arc<Kernel> {
    if args.len() > wal_arg_pos {
        Arc::new(Kernel::with_wal_path(args[wal_arg_pos].clone()))
    } else if let Ok(wal) = env::var("VERITAS_WAL") {
        Arc::new(Kernel::with_wal_path(wal))
    } else {
        Arc::new(Kernel::new())
    }
}

fn run_command(args: &[String]) -> Result<(), String> {
    match args[1].as_str() {
        "compile" => {
            if args.len() < 4 {
                return Err("Usage: veritas compile <input.vasm> <output.vmod>".into());
            }
            let src = fs::read_to_string(&args[2]).map_err(|e| format!("read asm: {}", e))?;
            let m = assemble_module(&src).map_err(|e| format!("assemble: {:?}", e))?;
            let encoded = m.encode_file().map_err(|e| format!("encode: {:?}", e))?;
            fs::write(&args[3], encoded).map_err(|e| format!("write: {}", e))?;
            println!("compiled -> {}", args[3]);
        }
        "run" => {
            if args.len() < 3 {
                return Err("Usage: veritas run <file.vmod> [wal_path]".into());
            }
            let kernel = get_kernel(args, 3);
            let bytes = fs::read(&args[2]).map_err(|e| format!("read vmod: {}", e))?;
            let mut loader = ModuleLoader::new();
            let name = loader.load_and_install(&bytes).map_err(|e| format!("load: {:?}", e))?;
            let loaded = loader.get_module(&name).ok_or("module not found after install")?;
            let outcome = Runtime::execute(&kernel, &loaded.image)
                .map_err(|e| format!("exec: {:?}", e))?;
            match outcome {
                ExecutionOutcome::Completed { pc, r0 } => {
                    println!("finished pc={} r0={}", pc, r0);
                    println!("objects in world: {}", kernel.list_object_ids().len());
                }
                ExecutionOutcome::Trapped { pc, reason, r0 } => {
                    eprintln!("trapped pc={} r0={} reason={:?}", pc, r0, reason);
                    println!("objects in world: {}", kernel.list_object_ids().len());
                    std::process::exit(1);
                }
            }
        }
        "info" => {
            if args.len() < 3 {
                return Err("Usage: veritas info <file.vmod>".into());
            }
            let bytes = fs::read(&args[2]).map_err(|e| format!("read vmod: {}", e))?;
            let m = ModuleImage::decode_file(&bytes).map_err(|e| format!("decode: {:?}", e))?;
            println!("name: {}", m.name);
            println!("version: {}.{}.{}", m.version.major, m.version.minor, m.version.patch);
            println!("instructions: {}", m.program_image.instructions.len());
        }
        "inspect" => {
            let wal = if args.len() > 2 && !args[2].starts_with("list") && !args[2].starts_with("object") {
                Some(args[2].clone())
            } else if let Ok(w) = env::var("VERITAS_WAL") {
                Some(w)
            } else {
                None
            };
            let sub = if wal.is_some() && args.len() > 3 {
                args[3].clone()
            } else if wal.is_none() && args.len() > 2 {
                args[2].clone()
            } else {
                "list".to_string()
            };
            let kernel = if let Some(ref path) = wal {
                Arc::new(Kernel::with_wal_path(path.clone()))
            } else {
                Arc::new(Kernel::new())
            };
            match sub.as_str() {
                "list" => {
                    let ids = kernel.list_object_ids();
                    if ids.is_empty() {
                        println!("(no objects)");
                    } else {
                        for id in &ids {
                            let state = kernel.get_object_state(*id);
                            let status = match state {
                                Some(s) => format!("{:?}", s),
                                None => "Unknown".into(),
                            };
                            println!("{} {}", id, status);
                        }
                    }
                }
                "object" => {
                    let obj_arg = if wal.is_some() && args.len() > 4 {
                        args[4].clone()
                    } else if wal.is_none() && args.len() > 3 {
                        args[3].clone()
                    } else {
                        return Err("Usage: veritas inspect object <id>".into());
                    };
                    let id: u64 = obj_arg.parse().map_err(|e| format!("invalid id: {}", e))?;
                    match kernel.get_object_state(id) {
                        Some(state) => println!("{} {:?}", id, state),
                        None => println!("{} not found", id),
                    }
                }
                _ => return Err(format!("unknown inspect subcommand: {}", sub)),
            }
        }
        "serve" => {
            let wal_path = if args.len() > 2 {
                args[2].clone()
            } else {
                "veritas_world.wal".to_string()
            };
            eprintln!("Starting veritasd on WAL: {}", wal_path);
            // exec into veritasd
            let status = process::Command::new("sh")
                .arg("-c")
                .arg(format!(
                    "VERITAS_WAL={} {}",
                    wal_path,
                    env::current_exe()
                        .unwrap()
                        .parent()
                        .unwrap()
                        .join("veritasd")
                        .display()
                ))
                .status()
                .map_err(|e| format!("failed to start veritasd: {}", e))?;
            if !status.success() {
                return Err("veritasd exited with error".into());
            }
        }
        _ => return Err(format!("unknown command: {}", args[1])),
    }
    Ok(())
}
