use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct Heap {
    memory: HashMap<u32, String>,
    next_free: u32,
}

impl Heap {
    pub fn allocate(&mut self, s: String) -> u32 {
        let ptr = self.next_free;
        self.next_free += 1;
        self.memory.insert(ptr, s);
        ptr
    }

    pub fn free(&mut self, ptr: u32) {
        self.memory.remove(&ptr);
    }

    pub fn string_getch(&self, ptr: u32, idx: u32) -> u32 {
        if let Some(s) = self.memory.get(&ptr) {
            s.as_bytes()[idx as usize] as u32
        } else {
            0
        }
    }

    pub fn string_getlen(&self, ptr: u32) -> u32 {
        if let Some(s) = self.memory.get(&ptr) {
            s.as_bytes().len() as u32
        } else {
            0
        }
    }
}
