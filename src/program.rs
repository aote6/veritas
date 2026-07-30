use crate::instruction::Instruction;

#[derive(Debug, Clone, Default)]
pub struct Program {
    pub instructions: Vec<Instruction>,
}

impl Program {
    pub fn new() -> Self {
        Self { instructions: Vec::new() }
    }

    pub fn push(mut self, inst: Instruction) -> Self {
        self.instructions.push(inst);
        self
    }

    pub fn len(&self) -> usize {
        self.instructions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }
}
