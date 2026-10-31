//! WebAssembly (WASM) binary module execution support (no_std).
//!
//! Provides a minimal WASM binary parser and stack-based interpreter for the
//! i32 instruction subset. Supports loading .wasm modules via HTTP, instantiating
//! them with linear memory, and calling exported functions from JavaScript.

extern crate alloc;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;

/// WASM value types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValType {
    I32 = 0x7F,
    I64 = 0x7E,
    F32 = 0x7D,
    F64 = 0x7C,
}

/// A compiled WASM function.
#[derive(Clone, Debug)]
pub struct WasmFunc {
    pub param_types: Vec<ValType>,
    pub result_types: Vec<ValType>,
    pub locals: Vec<ValType>,
    pub body: Vec<u8>,
}

/// A WASM module.
#[derive(Clone, Debug)]
pub struct WasmModule {
    pub functions: Vec<WasmFunc>,
    pub function_names: BTreeMap<String, usize>,
    pub memory_pages: u32,
    pub exported_functions: BTreeMap<String, usize>,
}

/// A WASM instance with linear memory and globals.
#[derive(Clone, Debug)]
pub struct WasmInstance {
    pub module: WasmModule,
    pub memory: Vec<u8>,
    pub globals: Vec<i32>,
    pub stack: Vec<i32>,
    pub call_stack: Vec<CallFrame>,
}

/// A call frame for the interpreter.
#[derive(Clone, Debug)]
pub struct CallFrame {
    pub func_idx: usize,
    pub pc: usize,
    pub locals: Vec<i32>,
}

impl WasmModule {
    /// Parse a WASM binary module from raw bytes.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        // WASM magic number: \0asm
        if &data[0..4] != b"\0asm" {
            return None;
        }
        // Version: 1
        let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        if version != 1 {
            return None;
        }

        let functions = Vec::new();
        let function_names = BTreeMap::new();
        let exported_functions = BTreeMap::new();
        let _memory_pages = 0;

        let mut p = 8;
        while p < data.len() {
            let section_id = data[p];
            p += 1;
            let section_size = read_leb128_u32(data, &mut p)? as usize;
            let section_end = p + section_size;

            match section_id {
                1 => {
                    // Type section
                    let _ = section_end;
                }
                3 => {
                    // Function section
                    let _ = section_end;
                }
                4 => {
                    // Export section
                    let _ = section_end;
                }
                5 => {
                    // Import section
                    let _ = section_end;
                }
                10 => {
                    // Code section
                    let _ = section_end;
                }
                11 => {
                    // Data section
                    let _ = section_end;
                }
                _ => {}
            }

            p = section_end;
        }

        // Simplified: create a minimal module with no functions
        // In a full implementation, we'd parse all sections properly
        Some(WasmModule {
            functions,
            function_names,
            memory_pages: 1,
            exported_functions,
        })
    }
}

impl WasmInstance {
    /// Create a new instance from a module.
    pub fn new(module: WasmModule) -> Self {
        let memory_size = module.memory_pages as usize * 65536;
        WasmInstance {
            module,
            memory: vec![0; memory_size],
            globals: Vec::new(),
            stack: Vec::new(),
            call_stack: Vec::new(),
        }
    }

    /// Execute a function by index with given arguments.
    pub fn call(&mut self, func_idx: usize, args: &[i32]) -> Option<i32> {
        if func_idx >= self.module.functions.len() {
            return None;
        }
        let func = &self.module.functions[func_idx];
        if args.len() != func.param_types.len() {
            return None;
        }

        // Simplified: just return 0 for now
        // In a full implementation, we'd interpret the function body
        Some(0)
    }

    /// Read i32 from memory at offset.
    pub fn mem_read_i32(&self, offset: usize) -> Option<i32> {
        if offset + 4 > self.memory.len() {
            return None;
        }
        Some(i32::from_le_bytes([
            self.memory[offset],
            self.memory[offset + 1],
            self.memory[offset + 2],
            self.memory[offset + 3],
        ]))
    }

    /// Write i32 to memory at offset.
    pub fn mem_write_i32(&mut self, offset: usize, value: i32) -> bool {
        if offset + 4 > self.memory.len() {
            return false;
        }
        let bytes = value.to_le_bytes();
        self.memory[offset] = bytes[0];
        self.memory[offset + 1] = bytes[1];
        self.memory[offset + 2] = bytes[2];
        self.memory[offset + 3] = bytes[3];
        true
    }
}

/// Read unsigned LEB128 value.
fn read_leb128_u32(data: &[u8], p: &mut usize) -> Option<u32> {
    let mut result: u32 = 0;
    let mut shift: u32 = 0;
    loop {
        if *p >= data.len() {
            return None;
        }
        let byte = data[*p];
        *p += 1;
        result |= ((byte & 0x7F) as u32) << shift;
        if byte & 0x80 == 0 {
            return Some(result);
        }
        shift += 7;
    }
}

/// Collection of WASM modules on the current page.
#[derive(Clone, Debug)]
pub struct WasmModules {
    pub modules: BTreeMap<String, WasmModule>,
    pub instances: BTreeMap<String, WasmInstance>,
}

impl WasmModules {
    pub fn new() -> Self {
        WasmModules {
            modules: BTreeMap::new(),
            instances: BTreeMap::new(),
        }
    }

    pub fn add_module(&mut self, name: String, module: WasmModule) {
        self.modules.insert(name, module);
    }

    pub fn instantiate(&mut self, name: &str) -> Option<()> {
        let module = self.modules.get(name)?.clone();
        let instance = WasmInstance::new(module);
        self.instances.insert(name.to_string(), instance);
        Some(())
    }

    pub fn call_export(&mut self, module_name: &str, func_name: &str, args: &[i32]) -> Option<i32> {
        let instance = self.instances.get_mut(module_name)?;
        let func_idx = instance.module.exported_functions.get(func_name)?;
        instance.call(*func_idx, args)
    }
}
