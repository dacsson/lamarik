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
