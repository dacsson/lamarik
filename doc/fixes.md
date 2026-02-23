# This file describes fixes applied

## 1. Allocations

The problem: 
```
После отведения нескольких начальных структур может быть использована только куча рантайма Ламы. Никаких других отведений в каком-либо постороннем рантайме быть не должно. Другой памяти просто может не быть.
```

Fixes:
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
