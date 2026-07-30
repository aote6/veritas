use std::collections::HashMap;
use crate::instruction::Instruction;
use crate::types::{VeritasError, AbortReason};

pub fn assemble(source: &str) -> Result<Vec<Instruction>, VeritasError> {
    let mut labels: HashMap<String, usize> = HashMap::new();
    let mut current_pc: usize = 0;

    // Pass 1: 收集 label 地址
    for line in source.lines() {
        let line = clean_line(line);
        if line.is_empty() { continue; }
        if line.ends_with(':') {
            labels.insert(line[..line.len()-1].trim().to_string(), current_pc);
            continue;
        }
        // 估算字节长度
        let mut dummy = HashMap::new();
        // 收集所有可能的 label 名并映射到 0
        for word in line.split_whitespace().skip(1).flat_map(|s| s.split(',')) {
            let w = word.trim();
            if !w.is_empty() && !w.starts_with('R') && !w.starts_with("0x") && w.parse::<u64>().is_err() {
                dummy.insert(w.to_string(), 0);
            }
        }
        let inst = parse_line(line, &dummy)?;
        current_pc += inst.encode()?.len();
    }

    // Pass 2: 生成指令
    let mut instructions = Vec::new();
    for line in source.lines() {
        let line = clean_line(line);
        if line.is_empty() || line.ends_with(':') { continue; }
        instructions.push(parse_line(line, &labels)?);
    }
    Ok(instructions)
}

fn clean_line(line: &str) -> &str {
    line.split(';').next().unwrap_or("").split("//").next().unwrap_or("").trim()
}

fn parse_reg(s: &str) -> Result<u8, VeritasError> {
    let s = s.trim().to_uppercase();
    if s.starts_with('R') {
        s[1..].parse::<u8>().map_err(|_| VeritasError::EngineError(format!("Bad reg: {}", s)))
    } else {
        Err(VeritasError::EngineError(format!("Bad reg: {}", s)))
    }
}

fn parse_u64(s: &str) -> Result<u64, VeritasError> {
    let s = s.trim();
    if s.starts_with("0x") || s.starts_with("0X") {
        u64::from_str_radix(&s[2..], 16).map_err(|_| VeritasError::EngineError(format!("Bad hex: {}", s)))
    } else {
        s.parse::<u64>().map_err(|_| VeritasError::EngineError(format!("Bad num: {}", s)))
    }
}

fn parse_target(s: &str, labels: &HashMap<String, usize>) -> Result<usize, VeritasError> {
    let s = s.trim();
    if let Some(&addr) = labels.get(s) {
        Ok(addr)
    } else {
        parse_u64(s).map(|v| v as usize)
    }
}

fn parse_line(line: &str, labels: &HashMap<String, usize>) -> Result<Instruction, VeritasError> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() { return Err(VeritasError::EngineError("Empty".into())); }
    let op = parts[0].to_uppercase();
    let joined = parts[1..].join(" ");
    let args: Vec<&str> = joined.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();

    match op.as_str() {
        "NOP" => Ok(Instruction::Nop),
        "HALT" => Ok(Instruction::Halt),
        "COMMIT" => Ok(Instruction::Commit),
        "ABORT" => Ok(Instruction::Abort { reason: AbortReason::WriteConflict }),
        "LOAD_CONST" => {
            Ok(Instruction::LoadConst { reg: parse_reg(args[0])?, val: parse_u64(args[1])? })
        }
        "ADD" => {
            Ok(Instruction::Add { dst: parse_reg(args[0])?, src1: parse_reg(args[1])?, src2: parse_reg(args[2])? })
        }
        "SUB" => {
            Ok(Instruction::Sub { dst: parse_reg(args[0])?, src1: parse_reg(args[1])?, src2: parse_reg(args[2])? })
        }
        "CMP" => {
            Ok(Instruction::Cmp { src1: parse_reg(args[0])?, src2: parse_reg(args[1])? })
        }
        "JMP" => {
            Ok(Instruction::Jmp { target: parse_target(args[0], labels)? })
        }
        "JZ" => {
            Ok(Instruction::Jz { target: parse_target(args[0], labels)? })
        }
        "JNZ" => {
            Ok(Instruction::Jnz { target: parse_target(args[0], labels)? })
        }
        "JN" => {
            Ok(Instruction::Jn { target: parse_target(args[0], labels)? })
        }
        "LOAD_STATE_U64" => {
            Ok(Instruction::LoadStateU64 { reg: parse_reg(args[0])?, state_id: parse_u64(args[1])? })
        }
        "LOAD_STATE_BYTES" => {
            Ok(Instruction::LoadStateBytes { reg: parse_reg(args[0])?, state_id: parse_u64(args[1])? })
        }
        "WRITE_REGISTER" => {
            Ok(Instruction::WriteRegister { state_id: parse_u64(args[0])?, reg: parse_reg(args[1])? })
        }
        _ => Err(VeritasError::EngineError(format!("Unknown op: {}", op))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assemble_simple() {
        let src = "
            LOAD_CONST R0, 10
            LOAD_CONST R1, 20
            ADD R2, R0, R1
            HALT
        ";
        let insts = assemble(src).unwrap();
        assert_eq!(insts.len(), 4);
        assert_eq!(insts[0], Instruction::LoadConst { reg: 0, val: 10 });
        assert_eq!(insts[3], Instruction::Halt);
    }

    #[test]
    fn test_assemble_with_labels() {
        let src = "
            LOAD_CONST R0, 3
        loop:
            SUB R0, R0, R1
            JNZ loop
            HALT
        ";
        let insts = assemble(src).unwrap();
        assert!(matches!(insts[2], Instruction::Jnz { .. }));
    }
}
