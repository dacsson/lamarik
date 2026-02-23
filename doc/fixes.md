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
