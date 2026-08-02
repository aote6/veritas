use std::env;
use std::fs;
use std::process;
use veritas_kernel::assembler::assemble_module;
use veritas_kernel::module::{ModuleImage, ModuleLoader};
use veritas_kernel::runtime::Runtime;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: veritas <compile|run|info> [args...]");
        process::exit(1);
    }

    let result = run_command(&args);
    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

fn run_command(args: &[String]) -> Result<(), String> {
    match args[1].as_str() {
        "compile" => {
            if args.len() < 4 {
                return Err("Usage: veritas compile <input.asm> <output.vmod>".into());
            }
            let src = fs::read_to_string(&args[2]).map_err(|e| format!("read asm: {}", e))?;
            let m = assemble_module(&src).map_err(|e| format!("assemble: {:?}", e))?;
            let encoded = m.encode_file().map_err(|e| format!("encode: {:?}", e))?;
            fs::write(&args[3], encoded).map_err(|e| format!("write: {}", e))?;
            println!("compiled -> {}", args[3]);
        }
        "run" => {
            if args.len() < 3 {
                return Err("Usage: veritas run <file.vmod>".into());
            }
            let bytes = fs::read(&args[2]).map_err(|e| format!("read vmod: {}", e))?;
            let mut loader = ModuleLoader::new();
            let name = loader.load_and_install(&bytes).map_err(|e| format!("load: {:?}", e))?;
            let loaded = loader.get_module(&name).ok_or("module not found after install")?;
            let (pc, r0) = Runtime::execute(&loaded.image).map_err(|e| format!("exec: {:?}", e))?;
            println!("finished pc={} r0={}", pc, r0);
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
        _ => return Err(format!("unknown command: {}", args[1])),
    }
    Ok(())
}
