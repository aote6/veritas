use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: veritas asm|run|inspect <file>");
        process::exit(1);
    }
    match args[1].as_str() {
        "asm" => {
            let input = &args[2];
            let output = args.get(4).expect("-o <out>");
            let src = fs::read_to_string(input).expect("read asm");
            let insts = veritas_kernel::assembler::assemble(&src).expect("assemble");
            let image = veritas_kernel::program::ProgramImage::new(insts);
            fs::write(output, image.encode().expect("encode")).expect("write");
            println!("OK: {} -> {}", input, output);
        }
        "run" => {
            let bin = fs::read(&args[2]).expect("read bin");
            let engine = veritas_kernel::engine::VeritasEngine::new();
            let mut machine = veritas_kernel::machine::Machine::new(&engine);
            machine.boot_bytes(&bin).expect("boot");
            machine.run().expect("run");
            println!("Status: {:?}", machine.status());
        }
        "inspect" => {
            let bin = fs::read(&args[2]).expect("read bin");
            let image = veritas_kernel::program::ProgramImage::decode(&bin).expect("decode");
            for inst in &image.instructions {
                let enc = inst.encode().expect("encode");
                println!("{:?} ({} bytes)", inst, enc.len());
            }
        }
        _ => eprintln!("unknown"),
    }
}
