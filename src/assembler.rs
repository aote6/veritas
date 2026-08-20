use crate::instruction::Instruction;
use crate::module::{ModuleImage, ModuleVersion};
use crate::program::ProgramImage;
use crate::types::VeritasError;
use std::collections::HashMap;

pub fn assemble(source: &str) -> Result<Vec<Instruction>, VeritasError> {
    let mut labels: HashMap<String, usize> = HashMap::new();
    let mut current_pc: usize = 0;

    // Pass 1: 收集 label 地址
    for line in source.lines() {
        let line = clean_line(line);
        if line.is_empty() {
            continue;
        }
        if line.ends_with(':') {
            labels.insert(line[..line.len() - 1].trim().to_string(), current_pc);
            continue;
        }
        let mut dummy = HashMap::new();
        for word in line.split_whitespace().skip(1).flat_map(|s| s.split(',')) {
            let w = word.trim();
            if !w.is_empty()
                && !w.starts_with('R')
                && !w.starts_with("0x")
                && w.parse::<u64>().is_err()
            {
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
        if line.is_empty() || line.ends_with(':') {
            continue;
        }
        instructions.push(parse_line(line, &labels)?);
    }
    Ok(instructions)
}

fn clean_line(line: &str) -> &str {
    line.split(";;").next().unwrap_or("").trim()
}

fn parse_reg(s: &str) -> Result<u8, VeritasError> {
    let s = s.trim().to_uppercase();
    if s.starts_with('R') {
        s[1..]
            .parse::<u8>()
            .map_err(|_| VeritasError::EngineError(format!("Bad reg: {}", s)))
    } else {
        Err(VeritasError::EngineError(format!("Bad reg: {}", s)))
    }
}

fn parse_operand(s: &str) -> Result<crate::instruction::Operand, VeritasError> {
    let t = s.trim().to_uppercase();
    if t.len() > 1 && t.starts_with('R') && t[1..].chars().all(|c| c.is_ascii_digit()) {
        parse_reg(s).map(crate::instruction::Operand::Register)
    } else {
        parse_u64(s).map(crate::instruction::Operand::Immediate)
    }
}

fn parse_u64(s: &str) -> Result<u64, VeritasError> {
    let s = s.trim();
    if s.starts_with("0x") || s.starts_with("0X") {
        u64::from_str_radix(&s[2..], 16)
            .map_err(|_| VeritasError::EngineError(format!("Bad hex: {}", s)))
    } else {
        s.parse::<u64>()
            .map_err(|_| VeritasError::EngineError(format!("Bad num: {}", s)))
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

/// 解析引号内的字符串，支持转义 \\" 和 \\\\
fn parse_quoted_string(s: &str) -> Result<String, VeritasError> {
    let s = s.trim();
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        let inner = &s[1..s.len() - 1];
        Ok(inner.replace("\\\"", "\"").replace("\\\\", "\\"))
    } else {
        Err(VeritasError::EngineError(format!(
            "Expected quoted string, got: {}",
            s
        )))
    }
}


fn parse_line(line: &str, labels: &HashMap<String, usize>) -> Result<Instruction, VeritasError> {
    let raw_parts: Vec<&str> = line.split_whitespace().collect();
    if raw_parts.is_empty() {
        return Err(VeritasError::EngineError("Empty".into()));
    }
    let op = raw_parts[0].to_uppercase();
    let joined = raw_parts[1..].join(" ");

    // 按逗号切分参数，但要保护引号内的逗号
    let args = split_args_keep_quotes(&joined);

    match op.as_str() {
        "NOP" => Ok(Instruction::Nop),
        "HALT" => Ok(Instruction::Halt),
        "RETURN" => Ok(Instruction::Return),

        "LOAD_CONST" => {
            if args.len() < 2 {
                return Err(VeritasError::EngineError(
                    "LOAD_CONST needs reg, val".into(),
                ));
            }
            Ok(Instruction::LoadConst {
                reg: parse_reg(args[0])?,
                val: parse_u64(args[1])?,
            })
        }
        "ADD" => {
            if args.len() < 3 {
                return Err(VeritasError::EngineError(
                    "ADD needs dst, src1, src2".into(),
                ));
            }
            Ok(Instruction::Add {
                dst: parse_reg(args[0])?,
                src1: parse_reg(args[1])?,
                src2: parse_reg(args[2])?,
            })
        }
        "SUB" => {
            if args.len() < 3 {
                return Err(VeritasError::EngineError(
                    "SUB needs dst, src1, src2".into(),
                ));
            }
            Ok(Instruction::Sub {
                dst: parse_reg(args[0])?,
                src1: parse_reg(args[1])?,
                src2: parse_reg(args[2])?,
            })
        }
        "CMP" => {
            if args.len() < 2 {
                return Err(VeritasError::EngineError("CMP needs src1, src2".into()));
            }
            Ok(Instruction::Cmp {
                src1: parse_reg(args[0])?,
                src2: parse_reg(args[1])?,
            })
        }
        "JMP" => {
            if args.is_empty() {
                return Err(VeritasError::EngineError("JMP needs target".into()));
            }
            Ok(Instruction::Jmp {
                target: parse_target(args[0], labels)?,
            })
        }
        "JZ" => {
            if args.is_empty() {
                return Err(VeritasError::EngineError("JZ needs target".into()));
            }
            Ok(Instruction::Jz {
                target: parse_target(args[0], labels)?,
            })
        }
        "JNZ" => {
            if args.is_empty() {
                return Err(VeritasError::EngineError("JNZ needs target".into()));
            }
            Ok(Instruction::Jnz {
                target: parse_target(args[0], labels)?,
            })
        }
        "JN" => {
            if args.is_empty() {
                return Err(VeritasError::EngineError("JN needs target".into()));
            }
            Ok(Instruction::Jn {
                target: parse_target(args[0], labels)?,
            })
        }
        "LOAD_STATE_U64" => {
            if args.len() < 2 {
                return Err(VeritasError::EngineError(
                    "LOAD_STATE_U64 needs reg, state_id".into(),
                ));
            }
            Ok(Instruction::LoadStateU64 {
                reg: parse_reg(args[0])?,
                state_id: parse_u64(args[1])?,
            })
        }
        "LOAD_STATE_BYTES" => {
            if args.len() < 2 {
                return Err(VeritasError::EngineError(
                    "LOAD_STATE_BYTES needs reg, state_id".into(),
                ));
            }
            Ok(Instruction::LoadStateBytes {
                reg: parse_reg(args[0])?,
                state_id: parse_u64(args[1])?,
            })
        }
        "WRITE_REGISTER" => {
            if args.len() < 2 {
                return Err(VeritasError::EngineError(
                    "WRITE_REGISTER needs state_id, reg".into(),
                ));
            }
            Ok(Instruction::WriteRegister {
                state_id: parse_u64(args[0])?,
                reg: parse_reg(args[1])?,
            })
        }

        // ===== 新增指令 =====
        "READ" => {
            if args.is_empty() {
                return Err(VeritasError::EngineError("READ needs state_id".into()));
            }
            Ok(Instruction::Read {
                state_id: parse_operand(args[0])?,
            })
        }
        "WRITE" => {
            if args.len() < 2 {
                return Err(VeritasError::EngineError(
                    "WRITE needs state_id, \"string\"".into(),
                ));
            }
            let payload = parse_quoted_string(args[1])?.into_bytes();
            Ok(Instruction::Write {
                state_id: parse_operand(args[0])?,
                payload,
            })
        }
        "CALL" => {
            if args.len() < 2 {
                return Err(VeritasError::EngineError(
                    "CALL needs object_id, entry_pc".into(),
                ));
            }
            Ok(Instruction::Call {
                object_id: parse_operand(args[0])?,
                entry_pc: parse_target(args[1], labels)?,
            })
        }
        "TRAP" => {
            if args.is_empty() {
                return Err(VeritasError::EngineError("TRAP needs service_id".into()));
            }
            Ok(Instruction::Trap {
                service_id: parse_u64(args[0])? as u8,
            })
        }
        "HOST_CALL" => {
            if args.is_empty() {
                return Err(VeritasError::EngineError("HOST_CALL needs call_id".into()));
            }
            Ok(Instruction::HostCall {
                call_id: parse_u64(args[0])? as u8,
            })
        }

        _ => Err(VeritasError::EngineError(format!("Unknown op: {}", op))),
    }
}

/// 按逗号切分参数，但保护双引号内的逗号不被切断
fn split_args_keep_quotes(input: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut in_quote = false;
    let mut start = 0;
    for (i, ch) in input.char_indices() {
        match ch {
            '"' => in_quote = !in_quote,
            ',' if !in_quote => {
                result.push(input[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    result.push(input[start..].trim());
    result.into_iter().filter(|s| !s.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruction::Operand;

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

    #[test]
    fn test_assemble_trap_and_native() {
        let src = r#"
            TRAP 0
            WRITE 100, "hello"
            READ 100
            CALL 100, 0
            RETURN
            TRAP 5
            HALT
        "#;
        let insts = assemble(src).unwrap();
        assert_eq!(insts.len(), 7);
        assert!(matches!(insts[0], Instruction::Trap { service_id: 0 }));
        assert!(matches!(
            insts[1],
            Instruction::Write {
                state_id: Operand::Immediate(100),
                ..
            }
        ));
        assert!(matches!(
            insts[2],
            Instruction::Read {
                state_id: Operand::Immediate(100)
            }
        ));
        assert!(matches!(
            insts[3],
            Instruction::Call {
                object_id: Operand::Immediate(100),
                entry_pc: 0
            }
        ));
        assert!(matches!(insts[4], Instruction::Return));
        assert!(matches!(insts[5], Instruction::Trap { service_id: 5 }));
        assert!(matches!(insts[6], Instruction::Halt));
    }

    #[test]
    fn test_legacy_kernel_mnemonics_rejected() {
        for mnemonic in [
            "OBJECT_BIRTH 0",
            "OBJECT_DEATH 0",
            "OBJECT_LINK 1, 2, owns",
            "OBJECT_UNLINK 1, 2",
            "OBJECT_FREEZE 0",
            "COMMIT",
            "EFFECT \"x\"",
            "SAVEPOINT \"s\"",
            "ROLLBACK_TO \"s\"",
            "CAPABILITY_GRANT 1, \"p\", 2",
            "ABORT",
        ] {
            assert!(
                assemble(mnemonic).is_err(),
                "legacy mnemonic should be rejected: {}",
                mnemonic
            );
        }
    }
}

pub fn assemble_module(source: &str) -> Result<ModuleImage, VeritasError> {
    let mut name = String::new();
    let mut version = ModuleVersion::new(0, 1, 0);
    let mut code_lines = Vec::new();

    for line in source.lines() {
        let cleaned = clean_line(line);
        if cleaned.is_empty() {
            continue;
        }
        let parts: Vec<&str> = cleaned.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0].to_lowercase().as_str() {
            "module" => {
                if parts.len() >= 2 {
                    name = parts[1].to_string();
                }
            }
            "version" => {
                if parts.len() >= 2 {
                    let v: Vec<&str> = parts[1].split('.').collect();
                    if v.len() == 3 {
                        version = ModuleVersion::new(
                            v[0].parse().unwrap_or(0),
                            v[1].parse().unwrap_or(0),
                            v[2].parse().unwrap_or(0),
                        );
                    }
                }
            }
            _ => {
                code_lines.push(line);
            }
        }
    }

    if name.is_empty() {
        return Err(VeritasError::EngineError("Missing module name".into()));
    }

    let instructions = assemble(&code_lines.join("\n"))?;
    let program_image = ProgramImage::new(instructions);
    Ok(ModuleImage::new(&name, version, program_image))
}

#[cfg(test)]
mod module_asm_tests {
    use super::*;

    #[test]
    fn test_assemble_module() {
        let src = concat!(
            "module math.add\n",
            "version 1.2.3\n",
            "entry_add:\n",
            "    LOAD_CONST R0, 1\n",
            "    HALT\n"
        );
        let m = assemble_module(src).unwrap();
        assert_eq!(m.name, "math.add");
        assert_eq!(m.version, ModuleVersion::new(1, 2, 3));
        assert_eq!(m.program_image.instructions.len(), 2);
    }
}
