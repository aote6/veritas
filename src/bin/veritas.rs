use std::env;
use std::fs;
use veritas_kernel::assembler::assemble_module;
use veritas_kernel::module::{ModuleImage, ModuleLoader};
use veritas_kernel::runtime::Runtime;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: veritas <compile|run|info> [args...]");
        return;
    }
    match args[1].as_str() {
        "compile" => {
            let src = fs::read_to_string(&args[2]).expect("read asm");
            let m = assemble_module(&src).expect("assemble");
            fs::write(&args[3], m.encode_file().expect("encode")).expect("write");
            println!("compiled -> {}", args[3]);
        }
        "run" => {
            let bytes = fs::read(&args[2]).expect("read vmod");
            let mut loader = ModuleLoader::new();
            let name = loader.load_and_install(&bytes).expect("load");
            let loaded = loader.get_module(&name).expect("get");
            let (pc, r0) = Runtime::execute(&loaded.image).expect("exec");
            println!("finished pc={} r0={}", pc, r0);
        }
        "info" => {
            let bytes = fs::read(&args[2]).expect("read vmod");
            let m = ModuleImage::decode_file(&bytes).expect("decode");
            println!("name: {}", m.name);
            println!("version: {}.{}.{}", m.version.major, m.version.minor, m.version.patch);
            println!("instructions: {}", m.program_image.instructions.len());
        }
        _ => println!("unknown command"),
    }
}
