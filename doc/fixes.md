# This file describes fixes applied

## 1. Allocations

The problem: 
```
После отведения нескольких начальных структур может быть использована только куча рантайма Ламы. Никаких других отведений в каком-либо постороннем рантайме быть не должно. Другой памяти просто может не быть.
```

### Fixes:
1. Remove *vector* allocations for captured variables in closure creation

Instead lets use a static array.

- Changes:
```rust
            Instruction::CLOSURE {
                offset,
                arity,
            } => unsafe {
                let offset_at = *offset as usize;

                // verify offset is within bounds
                #[cfg(feature = "runtime_checks")]
                if (*offset) < 0 || offset_at >= self.code_section_len {
                    return Err(InterpreterError::InvalidJumpOffset(
                        self.ip,
                        *offset,
                        self.code_section_len,
                    ));
                }
                
                // No heap allocation for vector of args
                let mut args = [0; MAX_ARG_LEN];

                // Push offset - which is a first element to args of Bsexp
                args[0] = *offset as i64;

                // Read captured variables description from code section
                for i in 0..*arity as usize {
                    let desc = CapturedVar {
                        rel: ValueRel::try_from(self.next::<u8>()?)
                            .map_err(|_| InterpreterError::InvalidValueRel)?,
                        index: self.next::<i32>()?,
                    };

                    // Push captures
                    match desc.rel {
                        ValueRel::Arg => {
                            let frame = FrameMetadata::get_from_stack(
                                &self.operand_stack,
                                self.frame_pointer,
                            )
                            .ok_or(
                                InterpreterError::NotEnoughArguments(
                                    "trying to create closure frame",
                                ),
                            )?;
                            let obj = frame
                                .get_arg_at(
                                    &self.operand_stack,
                                    self.frame_pointer,
                                    desc.index as usize,
                                )
                                .ok_or(InterpreterError::NotEnoughArguments(
                                    "trying to create closure frame",
                                ))?;
                            args[i + 1] = obj.raw();
                        }
                        ValueRel::Capture => {
                            let mut frame = FrameMetadata::get_from_stack(
                                &self.operand_stack,
                                self.frame_pointer,
                            )
                            .ok_or(
                                InterpreterError::NotEnoughArguments(
                                    "trying to create closure frame",
                                ),
                            )?;

                            let closure = frame
                                .get_closure(&mut self.operand_stack, self.frame_pointer)
                                .map_err(|_| {
                                    InterpreterError::InvalidLoadIndex(
                                        ValueRel::Capture,
                                        desc.index,
                                        1,
                                    )
                                })?;

                            let to_data = rtToData(
                                closure
                                    .as_ptr_mut()
                                    .ok_or(InterpreterError::InvalidObjectPointer)?,
                            );

                            let element = get_captured_variable(&*to_data, desc.index as usize);

                            args[i + 1] = element;
                        }
                        ValueRel::Global => {
                            let value = self.globals.get(desc.index as usize).ok_or(
                                InterpreterError::NotEnoughArguments(
                                    "trying to create closure frame",
                                ),
                            )?;
                            args[i + 1] = value.raw();
                        }
                        ValueRel::Local => {
                            let frame = FrameMetadata::get_from_stack(
                                &self.operand_stack,
                                self.frame_pointer,
                            )
                            .ok_or(
                                InterpreterError::NotEnoughArguments(
                                    "trying to create closure frame",
                                ),
                            )?;

                            let obj = frame
                                .get_local_at(
                                    &self.operand_stack,
                                    self.frame_pointer,
                                    desc.index as usize,
                                )
                                .ok_or(InterpreterError::NotEnoughArguments(
                                    "trying to create closure frame",
                                ))?;
                            args[i + 1] = obj.raw();
                        }
                    }
                }

                // Create a new closure object
                let closure = new_closure(&mut args[0..(*arity as usize + 1)]);

                self.push(
                    Object::try_from(closure)
                        .map_err(|_| InterpreterError::InvalidObjectPointer)?,
                )?;
```

- Regressions: [see here](regression_closure_no_heap.txt)

2. Remove *c-string* allocations for S-expressions and remove *vector* allocations for arguments for S-expressions

Instead of using `CString` we use a borrowed `CStr`, in std:
```
&CStr is to CString as &str is to String: the former in each pair are borrowing references; the latter are owned strings.
```

meaning we avoid allocations by using it.

- Changes:
```rust
            Instruction::SEXP { s_index, n_members } => {
                // + 1 for tag hash
                // ! Previous - args vas a `Vec<i64>`
                let mut args = [0; MAX_ARG_LEN];

                for i in (0..*n_members).rev() {
                    args[i as usize] = self.pop()?.raw();
                }

                let tag_u8 = self
                    .bf
                    .get_string_at_offset(*s_index as usize)
                    .map_err(|_| InterpreterError::StringIndexOutOfBounds)?;

                if cfg!(feature = "verbose") {
                    println!(
                        "[LOG][Instruction::SEXP] tag_u8: {:#?}, index {}",
                        tag_u8, s_index
                    );
                }
                let c_string = CStr::from_bytes_with_nul(tag_u8)
                    .map_err(|_| InterpreterError::InvalidCString)?;

                if cfg!(feature = "verbose") {
                    println!(
                        "[LOG][Instruction::SEXP] c_string: {}",
                        c_string.to_str().unwrap()
                    );
                }

                let sexp = new_sexp(c_string, &mut args[0..*n_members as usize + 1]);

                if cfg!(feature = "verbose") {
                    unsafe {
                        println!("[Log][SEXP] {:#?}", *rtToSexp(sexp));
                    }
                }

                self.push(
                    Object::try_from(sexp).map_err(|_| InterpreterError::InvalidObjectPointer)?,
                )?;
            }
```

```rust
// args here is a slice provided by caller, i.e. no allocations here
// ! Previous - args vas a `Vec<i64>`
#[inline(always)]
fn new_sexp(tag: &CStr, args: &mut [i64]) -> *mut c_void {
    unsafe {
        let tag_hash = if tag.to_bytes() == "cons".as_bytes() {
            CONS_TAG_HASH
        } else if tag.to_bytes() == "nil".as_bytes() {
            NIL_TAG_HASH
        } else {
            // WARNING: We are responsible for ensuring lifetime of the CStr
            //          but because LtagHash doesn't write to the CStr, i think it's safe to cast here
            LtagHash(tag.as_ptr() as *mut c_char)
        };

        if let Some(last) = args.last_mut() {
            *last = tag_hash;
        }

        Bsexp(
            args.as_mut_ptr(),        /* [args_1,...,arg_n, tag] */
            rtBox(args.len() as i64), /* n args */
        )
    }
}
```

We also remove *c string* allocations for STRING creation in the same manner

- Changes:
```rust
/// Create a new lama string.
/// Create a new lama string.
#[inline(always)]
fn new_string(bytes: &[u8]) -> Result<*mut c_void, Box<dyn std::error::Error>> {
    unsafe {
        // ! Previous - CString owning memory
        // let c_string = CString::new(bytes)?;
        // let as_ptr = c_string.into_raw();
        let c_string = CStr::from_bytes_with_nul(bytes)?;
        let as_ptr = c_string.as_ptr();
        
        // ! Previous - vector allocation
        // let mut slice = vec![as_ptr as i64];
        let mut slice: [i64; 1] = [as_ptr as i64];

        Ok(Bstring(slice.as_mut_ptr()))
    }
}
```

- Regressions: [see here](regression_cstr_insted_of_cstring.txt)

3. Remove *vector* allocations for arguments collection in `BEGIN` instruction

Instead lets use a static array.

- Changes:
```rust
            Instruction::BEGIN { args, locals } | Instruction::CBEGIN { args, locals } => {
                let mut closure_obj = Object::new_empty();
                let mut ret_ip = Object::new_empty();

                // Top object is either return_ip or a closure obj
                let mut obj = self
                    .pop()
                    .map_err(|_| InterpreterError::NotEnoughArguments("BEGIN"))?; // must be a closure

                // check for closure
                if let Some(lama_type) = obj.lama_type() {
                    // check for closure type
                    if lama_type == lama_type_CLOSURE {
                        closure_obj = obj;

                        // Save previous ip (provided by `CALL`)
                        ret_ip = self
                            .pop()
                            .map_err(|_| InterpreterError::NotEnoughArguments("BEGIN"))?;
                    }
                } else {
                    ret_ip = obj;
                }

                // Collect callee provided arguments in a static array
                // ! Previous - vector used
                let mut arguments: [Object; MAX_ARG_LEN] = array::repeat(Object::new_empty());
                for i in (0..*args as usize).rev() {
                    arguments[i] = self
                        .pop()
                        .map_err(|_| InterpreterError::NotEnoughArguments("BEGIN"))?
                }

                // Save previous frame pointer
                let ret_frame_pointer = self.frame_pointer;

                // Set new frame pointer as index into operand stack
                #[cfg(feature = "runtime_checks")]
                if self.operand_stack.is_empty() {
                    return Err(InterpreterError::NotEnoughArguments("BEGIN"));
                }
                self.frame_pointer = self.operand_stack.len() - 1;

                // Push closure object onto operand stack
                self.push(closure_obj)?;

                // Push arg and local count
                self.push(Object::new_unboxed(*args as i64))?;
                self.push(Object::new_unboxed(*locals as i64))?;

                // Push return frame pointer and ip
                // 1. Where to return in sack operand
                self.push(Object::new_unboxed(ret_frame_pointer as i64))?;
                // 2. Where to return in the bytecode after this call
                self.push(ret_ip)?;

                // Re-push arguments
                // By moving them from args array -> no additonal heap allocations 
                for (iarg, arg) in arguments.into_iter().enumerate() {
                    if iarg > *args as usize {
                        break;
                    }
                    self.push(arg)?;
                }

                // Initialize local variables with 0
                // We create them as boxed objects
                for _ in 0..*locals {
                    self.push(Object::new_boxed(0))?;
                }
            }
```

- Regressions: [see here](regression_begin_arguments_no_vec.txt)

4. Remove *vector* allocation for builtin functions that collect arguments

Instead use a static array.

- Changes:
```rust
// elements is now a mutable reference to a slice of i64 - no need to allocate a vector
#[inline(always)]
fn new_array(elements: &mut [i64]) -> *mut c_void {
    unsafe {
        Barray(
            elements.as_mut_ptr(),        /* [args_1,...,arg_n, tag] */
            rtBox(elements.len() as i64), /* n args */
        )
    }
}
```

```rust
Builtin::Barray => {
    let length =
        n.ok_or(InterpreterError::InvalidLengthForArray)? as usize;
    
    // Static array to hold the arguments
    // ! Previous - vector used
    let mut elements = [0; MAX_ARG_LEN];
    for i in (0..length).rev() {
        elements[i as usize] = self.pop()?.raw();
    }

    let array = new_array(&mut elements[..length]);

    self.push(
        Object::try_from(array)
            .map_err(|_| InterpreterError::InvalidObjectPointer)?,
    )?;
}
```

```rust
Builtin::Lstring => {
    let obj = self.pop()?;
    
    // Static array to hold the arguments
    // ! Previous - vector used
    let mut slice: [i64; 1] = [obj.raw()];

    unsafe {
        let ptr = Lstring(slice.as_mut_ptr());
```

```rust
Instruction::FAIL { line, column } => unsafe {
    let obj = self.pop()?;
    
    // static array of one element insted of vector
    let ptr = Lstring([obj.raw()].as_mut_ptr());
```

- Regressions: [see here](regression_builtin_funcs_no_vec.txt)

5. Operand stack vector allocation

About operand stack:
```
Разумеется, вы могли бы в самом начале отвести несколько (очень немного) стандарных векторов и сразу сказать им reserve или resize, если вам очень хочется писать на идиоматическом С++
```

Yes, my operand stack is a vector (all be it a Rust vector, not a C++ vector as stated above :-) ), but we make sure to initialize it with a good enough capacity:
```rust
const MAX_OPERAND_STACK_SIZE: usize = 0x7fffffff;

let mut operand_stack = Vec::with_capacity(MAX_OPERAND_STACK_SIZE);
```

This, obviously, allows us to negate all reallocations during interpretation.

### Proof of allocation/memory usage

Again, problem:
```
После отведения нескольких начальных структур может быть использована только куча рантайма Ламы. Никаких других отведений в каком-либо постороннем рантайме быть не должно. Другой памяти просто может не быть.
```

And:
```
Даже при использовании С++ вы не модете использовать какие-либо структуры, явно или неявно динамический отводящие память в рантайме С++ после начала интерпретации файла.
```

Let's see where in our interpreter we do allocations and prove that with the fixes above we do *not* allocate memory in Rust runtime *during interpretation*.

I used `valgrind --tool=massif ./target/release/lama-rs -l ../Lama/Sort.bc` to track heap usage, you can see the output [here](valgrind-massif-output.txt), but basically we can grep for our `eval` function and see that there is no heap allocations in Rust side:

```
~/Uni/VirtualMachines/lama-rs  =>  cat doc/valgrind-massif-output.txt | grep 'eval'
~/Uni/VirtualMachines/lama-rs  =>
```

Additionally we can check for allocations in main modules: `disasm` and `interpeter`:

```
~/Uni/VirtualMachines/lama-rs  =>  cat doc/valgrind-massif-output.txt | grep 'lama_rs::disasm'
|   ->33.51% (764B) 0x146AD0: lama_rs::disasm::Bytefile::parse (in /home/safonoff/Uni/VirtualMachines/lama-rs/target/release/lama-rs)
~/Uni/VirtualMachines/lama-rs  =>  cat doc/valgrind-massif-output.txt | grep 'lama_rs::interpreter'
->100.00% (34,359,738,352B) 0x140357: lama_rs::interpreter::Interpreter::new (in /home/safonoff/Uni/VirtualMachines/lama-rs/target/release/lama-rs)
```

We see that interpreter does not allocate memory in Rust runtime during interpretation, only during its creation (operand stack reservation).

## 2. Tools splitting

Instead of having interpreter that does all the work, we should split it into:
- interpreter with runtime checks
- frequency analysis tool
- static bytecode verifyer

```
Замечание №1: домашние работы 2-4 не могут быть частями одного исполняемого файла. №2 - это интерпретатор. №3 - это отдельная утилита для анализа идиом, которая обычно пишется другим человеком и часто другой компанией, поэтоум должна минимально зависеть от кодировки набора инструкций. №4 - это встроенный в интерпреттаор верификатор, который модифицирует байткод в памяти и в котоырй выноситься большинство проверок времени исполнения. Интерпретаторы #2 и #4 разные.
```

1. Split tools into separate project

This is done via splitting this cargo workspace into multiple crates:
- `lamacore` - core library for working with bytefiles
- `lamarik` - first interpreter
- `lamanyzer` - frequency analyzer
- `lamarifyer` - static bytecode verifyer

Each crate is its own project, with its own dependencies, build scripts and executable. 

The `lamanyzer` crate depends on `lamacore` for bytefile parsing, in `lamanyzer/Cargo.toml`:
```toml
[dependencies]
lamacore = { path = "../lamacore" }
```

The `lamarifyer` crate depends on `lamarik` for interpreter and `lamacore` for bytefile parsing, in `lamarifyer/Cargo.toml`:
```toml
[dependencies]
lamarik = { path = "../lamarik" }
lamacore = { path = "../lamacore" }
```

So, after building a workspace, via `cargo build --release` in build directory we have three separate executables:
```
~/Uni/VirtualMachines/lama-rs  =>  cargo build --release
~/Uni/VirtualMachines/lama-rs  =>  ls target/release/
build  examples     lamanyzer    lamarifyer    lamarik    liblamacore.d     liblamarik.d
deps   incremental  lamanyzer.d  lamarifyer.d  lamarik.d  liblamacore.rlib  liblamarik.rlib
```

2. Remove analyzer from first interpreter 

- Changes:
Removed `analyzer.rs` from first interpreter, now it just includes runtime checks enabled by `cargo build --release --features="runtime_checks"`

Additionally fixed this:
```
2. Что, если на вход вашей программе подать файл длиной в 1 ТБ? Выдаваемая диагностика должна быть конкретной.
    let mut content = Vec::new();
    file.read_to_end(&mut content)?;
```

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Check file size
    let metadata = std::fs::metadata(&args.lama_file).map_err(|err| {
        eprintln!("{}", err);
        err
    })?;
    if metadata.len() >= MAX_FILE_SIZE {
        return Err(InterpreterError::FileIsTooLarge(
            args.lama_file.to_string(),
            metadata.len(),
        ))
        .map_err(|err| {
            eprintln!("{}", err);
            err
        })?;
    }
    ...
```

Diagnostic output:
```
~/Uni/VirtualMachines/lama-rs  =>  ./target/release/lamarik -l ~/Downloads/sc-dt-2025.11-Ubuntu22.04-x86_64-internal-g86e02dcc-d251024-172639.tar.gz
File /home/safonoff/Downloads/sc-dt-2025.11-Ubuntu22.04-x86_64-internal-g86e02dcc-d251024-172639.tar.gz is too large: 1206037184, max is 1GB
```

## 3. Missing checks and diagnostics

1. No checks in `CJMP`:

```rust
// ! Previous
Instruction::CJMP { offset, kind } => match kind {
    CompareJumpKind::ISNONZERO => {
        let obj = self.pop()?;
        let value = obj.unwrap();
    
        if value != 0 {
            self.ip = *offset as usize;
        }
    }
    CompareJumpKind::ISZERO => {
        let obj = self.pop()?;
        let value = obj.unwrap();
    
        if value == 0 {
            self.ip = *offset as usize;
        }
    }
    },
```

Comment:
```
Нет, я не вижу тут проверки.
```

- Changes:
```rust
Instruction::CJMP { offset, kind } => {
    let offset_at = *offset as usize;

    // verify offset is within bounds
    #[cfg(feature = "runtime_checks")]
    if (*offset) < 0 || offset_at >= self.code_section_len {
        return Err(InterpreterError::InvalidJumpOffset(
            self.ip,
            *offset,
            self.code_section_len,
        ));
    }

    match kind {
        CompareJumpKind::ISNONZERO => {
            let obj = self.pop()?;
            let value = obj.unwrap();

            if value != 0 {
                self.ip = offset_at;
            }
        }
        CompareJumpKind::ISZERO => {
            let obj = self.pop()?;
            let value = obj.unwrap();

            if value == 0 {
                self.ip = offset_at;
            }
        }
    }
}
```

2. Diagnostics doesn't tell bytefile offset

Basically:
```
Индексы переменных и смещения в переходах тоже нужно проверять на корректность и выдавать конкретную диагностику с привязкой к расположению байткода в файле.
```

- Changes:
```rust
/// Main interpreter loop
/// Main interpreter loop
pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
    while self.ip < self.code_section_len {
        let encoding = self.next::<u8>()?;
        let instr = self.decode(encoding)?;

        if cfg!(feature = "verbose") {
            println!("[LOG] IP {} BYTE {} INSTR {:?}", self.ip, encoding, instr);
        }

        self.eval(&instr)
            .map_err(|e| -> Box<dyn std::error::Error> {
                let global_offset = std::mem::size_of::<i32>()
                    + std::mem::size_of::<i32>()
                    + std::mem::size_of::<i32>()
                    + (std::mem::size_of::<i32>() * 2 * self.bf.public_symbols_number as usize)
                    + self.bf.stringtab_size as usize
                    + self.ip;

                format!("Error at offset {}: {}", global_offset, e).into()
            })?;

        // HACK: if we encounter END instruction, while in frame 0
        //       (a.k.a main function) we exit the interpreter
        if let Instruction::END = instr
            && self.frame_pointer == 0
        {
            break;
        }
    }

    Ok(())
}
```

I modifyed a compiled file to showcase how errors look now:
```
~/Uni/VirtualMachines/lama-rs  =>  ./target/release/lamarik -l testBAD.bc
> 5
Error at offset 45: Invalid store index 10/3 for global variable
```

## 4. Shared layer for tools

We shouldn't repeat outselfs in all those different tools, thus we need a shared layer of basic operations on bytecode file: parsing and decoding.

Idea originated from:
```
Нет, в анализаторе вам не придется самим декодировать инструкции. Нужно завести минимальный разумный программынй интерфейс и реализлвать его с максимальным повторным использованием кода существующего дизассемблера byterun.
```

This layer is a `lamacore` crate (library), it exposes:
```
└── src
    ├── bytecode.rs   <- Lama VM bytecode description
    ├── decoder.rs    <- bytecode instruction decoder
    ├── bytefile.rs   <- bytefile parsing 
    ├── lib.rs        <- exposes modules
    └── numeric.rs    <- util traits 
```

All other tools will use this crate.

## 5. Idiom analyzer

1. Analyzer doesnt walk reachable code

Analyzer should start from public symbols (can be more then one!) and then walk to every reachable offset.

Comments:
```
Анализатор идиом вообще делает что-то странное, игнорируя достижимость кода и точки входа потока управления.
```

```
Анализировать нужно весь достижимый от точек входа (смещений публичных символов) код.
```

```
Там ограничение по памяти 16х размер входного файла на все структуры данных в сумме.
```

- Changes: algorithm is changed entirely, please look at `lamanyzer/src/analyzer.rs`, for memory usage info look at `lamanyzer/README.md`

## 6. Static checks

Should change bytecode:
```
Модификации непосредственного операнда BEGIN/CBEGIN в последнем задании тоже не заметил.
```

- Changes:

New file: `lamarifyer/src/verifyer.rs` with static checks

Only reachable instructions are traversed.

Memorisation:
```rust
for (offset, depth) in res.0.stack_depths.iter().enumerate() {
    if *depth != 0 {
        // println!("Stack depth: {}", depth);

        let begin_instr_bytes = &new_bytefile.code_section[offset - 8..offset - 4];
        let mut payload = u32::from_le_bytes(begin_instr_bytes.try_into().unwrap());
        payload |= (depth.to_le() as u32) << 16;
        new_bytefile.code_section[offset - 8..offset - 4]
            .copy_from_slice(&payload.to_le_bytes());
    }
}
```

Usage:
```rust
Instruction::BEGIN {
    args: payload,
    locals: payload2,
}
| Instruction::CBEGIN {
    args: payload,
    locals: payload2,
} => {
    let stack_size_for_function = payload >> 16;
    let args = (payload & 0xFFFF) as usize;

    let reachable = payload2 >> 16;
    let locals = (payload2 & 0xFFFF) as usize;

    if let Instruction::BEGIN { .. } = instr {
        if stack_size_for_function <= 0 {
            return Err(InterpreterError::StackOverflow);
        }
    }

    if self.operand_stack.len() + stack_size_for_function as usize
        > MAX_OPERAND_STACK_SIZE
    {
        return Err(InterpreterError::StackOverflow);
    }
```

# Fixes pt2

## Interpreter

### Allocations pt2 

```
Не знаю способа избавиться от отведений памяти в Rust'е, кроме как выключив библиотеку std и далее все писать вручную непохожим на Rust образом. Отсутствие отведений в трассе показыввет лишь, что их не было при данном прогоне. Никакое количество прогонов не гарантирует полного покрытия.
```

- Changes:
```rust
// in lib.rs:
#![no_std]
```

```rust
// in interpreter.rs:
#[repr(align(16))]
struct OperandStack([Object; MAX_OPERAND_STACK_SIZE]);

pub struct Interpreter {
    operand_stack: OperandStack, // <- not a vector
    operand_stack_len: usize,
    frame_pointer: usize,
    // Bytefile decoder
    decoder: Decoder,
    /// Code section length
    code_section_len: usize,
    /// Globals length
    global_areas_size: usize,
}
```

### Globals are not tracked by runtime

```
globals: Vec<Object>, кстати, тоже работать не может
```

- Changes:
```rust
// Globals now are part of operand stack:
pub fn new... {
    ...
    // Put globals at the start of operand stack
    let global_areas_size = decoder.bf.global_area_size as usize;
    for i in 0..global_areas_size {
        operand_stack.0[i] = Object::new_empty();
    }
    ...
}
```

### Argument collection:

```
Нет, статические массивы до максимума длины тоже не подойдут. Найдите способ обойтись без них.
```

- Changes:
```rust
// Take array building for example:
Builtin::Barray => unsafe {
    let length =
        n.ok_or(InterpreterError::InvalidLengthForArray)? as usize;
    
    // NEW: now we just borrow the stack elements directly
    //      no allocation needed, that is just a slice of the operand stack
    let borrow_operand_stack_elements = &mut self.operand_stack.0
        [self.operand_stack_len - length + 1..=self.operand_stack_len];
    let array = new_array(borrow_operand_stack_elements);

    // remove args
    for _ in 0..length {
        self.pop()?;
    }

    self.push(
        Object::try_from(array)
            .map_err(|_| InterpreterError::InvalidObjectPointer)?,
    )?;
},
```

### Regressions with `no_std`

- Lamarik: [see here](regression_no_std_lamarik)
- Lamarifyer: [see here](regression_no_std_lamarifyer)

## Analyzer

### Public symbols checks

```
Проверяете ли вы корректность смещений публичных символов? 
```

- Changed:
```rust
// In bytefile parsing:
// Check public symbols offsets are within bounds
for (s_index, offset) in &public_symbols {
    if *offset >= code_section.len() as u32 {
        return Err(BytefileError::InvalidPublicSymbolOffset(
            *offset,
            code_section.len() as u32,
        ));
    }

    if *s_index >= string_table.len() as u32 {
        return Err(BytefileError::InvalidStringIndexInStringTable);
    }
}
```

### Skip already visited offsets in public symbols

```
Что, если многие публичные символы ссылаются на одно смещение? Не следует ли перед добавлением элемента в очередь всегда проверять, не помечен ли он, и если помечен, сразу помечать и потом помещать в очередь? 
```

- Changes:
```rust
// In verifyer.rs:
for (_, offset) in &self.decoder.bf.public_symbols {
    if !worklist.contains(offset) {
        worklist.push_back(*offset);
    }
}
```

### Unnecessary deduplication

```
А зачем тут dedup? 
addresses.dedup()
```

- Changes:
```rust
// In verifyer.rs added checks so we dont need dedup anymore:
// example
if !worklist.contains(&(self.decoder.ip as u32)) {
    worklist.push_back(self.decoder.ip as u32);
}
```

### HashMap is too expensive

```
Ваши структуры, например, хеш-таблицы потребляют очень много памяти. У каждого отдельного блока в куче заголовок 4 слова.
```

Changed HashMap to Vec:
| Data structure | Approx. per el memory  |
|----------------|----------------------------------------------|
| `HashMap<u16, u32>` | **approx  16 B** (key 2 B + value 4 B + stored hash 8 B, padded to 16 B) |
| `Vec<(u16, u32)>`            | **approx  8 B** (key 2 B + value 4 B, padded to 8 B) |

This halves the memory usage.

- Changes:
```rust
// In analyzer.rs:
/// On strategy used:
/// Each instruction is assigned a unique ID upon encountering it,
/// so we give a compact numeric representation to each instruction.
/// Then each ID is used as the key in the frequency map,
/// which is actually just a vector of (ID, count) pairs.
pub struct Frequency {
    frequency: Vec<(u16, u32)>,
    instruction_to_id: Vec<Instruction>,
}
```

```rust
// Packing exmaple
let key = (id1 as u16) << 8 | id2 as u16;
```
