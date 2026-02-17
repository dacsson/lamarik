use crate::{
    __gc_init, Bsexp, LtagHash, alloc_sexp, get_array_el, get_object_content_ptr, get_sexp_el,
    isUnboxed, lama_type_SEXP, lama_type_STRING, new_array, new_sexp, rtBox, rtLen, rtSexpEl,
    rtTag, rtToData, rtToSexp, sexp,
};

use super::*;
use std::{ffi::CStr, os::raw::c_void, ptr};

/// Test minimal decoder functionality of translating bytecode to instructions
#[test]
fn test_decoder_minimal() -> Result<(), Box<dyn std::error::Error>> {
    // ~ =>  xxd dump/test1.bc
    // 00000000: 0500 0000 0100 0000 0100 0000 0000 0000  ................
    // 00000010: 0000 0000 6d61 696e 0052 0200 0000 0000  ....main.R......
    // 00000020: 0000 1002 0000 0010 0300 0000 015a 0100  .............Z..
    // 00000030: 0000 4000 0000 0018 5a02 0000 005a 0400  ..@.....Z....Z..
    // 00000040: 0000 2000 0000 0071 16ff                 .. ....q..
    let data: Vec<u8> = vec![
        0x05, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x52, 0x02, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x10, 0x02, 0x00, 0x00, 0x00, 0x10, 0x03, 0x00, 0x00, 0x00, 0x01,
        0x5a, 0x01, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x18, 0x5a, 0x02, 0x00, 0x00,
        0x00, 0x5a, 0x04, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x00, 0x71, 0x16, 0xff,
    ];

    let bytefile: Bytefile = Bytefile::parse(data)?;

    let mut interp = Interpreter::new(
        bytefile,
        InterpreterOpts {
            parse_only: true,
            verbose: true,
        },
    );

    interp.run()?;

    // We expect the following instructions to be generated:
    // /LABEL ("main")
    // BEGIN ("main", 2, 0, [], [], [])
    // /SLABEL ("L1")
    // CONST (2)
    // CONST (3)
    // BINOP ("+")
    // /LINE (1)
    // ST (Global ("z"))
    // DROP
    // /LINE (2)
    // /LINE (4)
    // LD (Global ("z"))
    // CALL ("Lwrite", 1, false)
    // /SLABEL ("L2")
    // END

    assert_eq!(
        interp.instructions[0],
        Instruction::BEGIN { args: 2, locals: 0 }
    );
    assert_eq!(interp.instructions[1], Instruction::CONST { value: 2 });
    assert_eq!(interp.instructions[2], Instruction::CONST { value: 3 });
    assert_eq!(interp.instructions[3], Instruction::BINOP { op: Op::ADD });
    assert_eq!(
        interp.instructions[5],
        Instruction::STORE {
            rel: ValueRel::Global,
            index: 0
        }
    );
    assert_eq!(interp.instructions[6], Instruction::DROP);
    assert_eq!(
        interp.instructions[9],
        Instruction::LOAD {
            rel: ValueRel::Global,
            index: 0
        }
    );
    assert_eq!(
        interp.instructions[10],
        Instruction::CALL {
            offset: None,
            n: None,
            name: Some(Builtin::Lwrite),
            builtin: true
        }
    );
    assert_eq!(interp.instructions[11], Instruction::END);

    Ok(())
}

/// Test minimal evaluation functionality of the interpreter
#[test]
fn test_binops_eval() -> Result<(), Box<dyn std::error::Error>> {
    let mut programs = Vec::new();
    for i in 1u8..=13u8 {
        let program = vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 3 },
            Instruction::BINOP {
                op: Op::try_from(i).unwrap(),
            },
        ];
        programs.push(program);
    }

    // tests on 0
    programs.push(vec![
        Instruction::CONST { value: 0 },
        Instruction::CONST { value: 0 },
        Instruction::BINOP { op: Op::AND },
    ]);

    programs.push(vec![
        Instruction::CONST { value: 0 },
        Instruction::CONST { value: 1 },
        Instruction::BINOP { op: Op::OR },
    ]);

    programs.push(vec![
        Instruction::CONST { value: 0 },
        Instruction::CONST { value: 0 },
        Instruction::BINOP { op: Op::OR },
    ]);

    // equality
    programs.push(vec![
        Instruction::CONST { value: 1 },
        Instruction::CONST { value: 1 },
        Instruction::BINOP { op: Op::EQ },
    ]);

    programs.push(vec![
        Instruction::CONST { value: 1 },
        Instruction::CONST { value: 1 },
        Instruction::BINOP { op: Op::NEQ },
    ]);

    let expected_results = vec![
        5,  // 2 + 3 = 5
        -1, // 2 - 3 = 5
        6,  // 2 * 3 = 6
        0,  // 2 / 3 = 0
        2,  // 2 % 3 = 2
        1,  // 2 < 3 = 1
        1,  // 2 <= 3 = 1
        0,  // 2 > 3 = 0
        0,  // 2 >= 3 = 0
        0,  // 2 == 3 = 0
        1,  // 2 != 3 = 1
        1,  // 2 && 3 = 1
        1,  // 2 != 3 = 1
        0,  // 0 && 0 = 0
        1,  // 0 || 1 = 1
        0,  // 0 || 0 = 0
        1,  // 1 == 1 = 1
        0,  // 1 != 1 = 0
    ];

    assert_eq!(programs.len(), expected_results.len());

    for (program, expected) in programs.into_iter().zip(expected_results) {
        let mut interp = Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));
        interp.run_on_program(program)?;

        let top = interp.pop().unwrap();

        assert_eq!(top.unwrap(), expected);
    }

    Ok(())
}

#[test]
fn test_string_eval() -> Result<(), Box<dyn std::error::Error>> {
    let mut programs = Vec::new();

    // "main"
    programs.push(vec![Instruction::STRING { index: 0 }]);

    // "Hello"
    programs.push(vec![Instruction::STRING { index: 5 }]);

    let expected_results = vec![CString::new("main")?, CString::new("Hello, world!")?];

    assert_eq!(programs.len(), expected_results.len());

    for (program, expected) in programs.into_iter().zip(expected_results) {
        let mut interp = Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));

        interp.bf.put_string(CString::new("main")?);
        interp.bf.put_string(CString::new("Hello, world!")?);

        println!("{}", interp.bf);

        interp.run_on_program(program)?;

        let mut obj = interp.pop().unwrap();

        assert_eq!(obj.lama_type(), Some(lama_type_STRING));

        unsafe {
            let as_ptr = obj.as_ptr_mut().ok_or("Failed to get pointer")?;
            let contents = (*rtToData(as_ptr)).contents.as_ptr();
            let c_string_again = CStr::from_ptr(contents);

            assert_eq!(*c_string_again, expected);
        }
    }

    Ok(())
}

#[test]
fn test_sexp_cons_nil_eval() -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        __gc_init();
    }

    let mut programs = Vec::new();

    programs.push(vec![
        // Nil()
        Instruction::SEXP {
            s_index: 5,
            n_members: 0,
        },
    ]);

    programs.push(vec![
        Instruction::CONST { value: 1 },
        // Nil()
        Instruction::SEXP {
            s_index: 5,
            n_members: 0,
        },
        // Cons(1, Nil())
        Instruction::SEXP {
            s_index: 0,
            n_members: 2,
        },
    ]);

    programs.push(vec![
        Instruction::CONST { value: 1 },
        Instruction::CONST { value: 2 },
        // Nil()
        Instruction::SEXP {
            s_index: 5,
            n_members: 0,
        },
        // Cons(1, Nil())
        Instruction::SEXP {
            s_index: 0,
            n_members: 2,
        },
        // Cons(2, Cons(1, Nil()))
        Instruction::SEXP {
            s_index: 0,
            n_members: 2,
        },
    ]);

    struct Expect {
        tag: i64,
        contents: Vec<i64>,
    }

    // checking tags and contents
    let expected_results = vec![
        Expect {
            tag: unsafe { rtUnbox(LtagHash(CString::new("Nil")?.into_raw())) },
            contents: vec![],
        },
        Expect {
            tag: unsafe { rtUnbox(LtagHash(CString::new("Cons")?.into_raw())) },
            contents: vec![1],
        },
        Expect {
            tag: unsafe { rtUnbox(LtagHash(CString::new("Cons")?.into_raw())) },
            contents: vec![1, 2],
        },
    ];

    assert_eq!(programs.len(), expected_results.len());

    for (program, expected) in programs.into_iter().zip(expected_results) {
        let mut interp = Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));

        interp.bf.put_string(CString::new("Cons")?);
        interp.bf.put_string(CString::new("Nil")?);

        println!("{}", interp.bf);

        interp.run_on_program(program)?;

        let mut obj = interp.pop().unwrap();
        let sexp = obj.as_ptr_mut::<c_void>().unwrap();

        assert_eq!(obj.lama_type(), Some(lama_type_SEXP));

        unsafe {
            assert_eq!((*rtToSexp(sexp)).tag, expected.tag as u64);

            for (i, el) in expected.contents.iter().enumerate() {
                // skip tags
                if i % 2 != 0 {
                    continue;
                }
                assert_eq!(rtUnbox(get_sexp_el(&*rtToSexp(sexp), i)), *el);
            }
        }
    }

    Ok(())
}

#[test]
fn test_invalid_jump_offset() -> Result<(), Box<dyn std::error::Error>> {
    let mut programs = Vec::new();

    // negative jump offset
    programs.push(vec![Instruction::JMP { offset: -1 }]);

    // out of bounds jump offset
    programs.push(vec![Instruction::JMP { offset: 10000 }]);

    let expected_results = vec![
        Err(InterpreterError::InvalidJumpOffset(0, -1, 100)),
        Err(InterpreterError::InvalidJumpOffset(0, 10000, 100)),
    ];

    assert_eq!(programs.len(), expected_results.len());

    for (program, expected) in programs.into_iter().zip(expected_results) {
        let mut interp = Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));
        let result = interp.run_on_program(program);

        assert!(result.is_err());
        assert_eq!(result, expected);
    }

    Ok(())
}

#[test]
fn test_invalid_frame_move() -> Result<(), Box<dyn std::error::Error>> {
    let mut programs = Vec::new();

    // End without begin
    programs.push(vec![Instruction::CONST { value: 1 }, Instruction::END]);

    // Not enough arguments for frame metadata
    programs.push(vec![Instruction::BEGIN {
        args: 10,
        locals: 0,
    }]);

    let expected_results = vec![
        Err(InterpreterError::NotEnoughArguments("END")),
        Err(InterpreterError::NotEnoughArguments("BEGIN")),
    ];

    assert_eq!(programs.len(), expected_results.len());

    for (program, expected) in programs.into_iter().zip(expected_results) {
        let mut interp = Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));
        interp.operand_stack.clear();
        let result = interp.run_on_program(program);

        assert!(result.is_err());
        assert_eq!(result, expected);
    }

    Ok(())
}

#[test]
fn test_frame_move_args_and_locals() -> Result<(), Box<dyn std::error::Error>> {
    let mut programs = Vec::new();

    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 }, // return ip
        Instruction::BEGIN { args: 2, locals: 2 },
    ]);

    // Local variables assignment
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 }, // return ip
        Instruction::BEGIN { args: 2, locals: 2 },
        Instruction::CONST { value: 3 },
        Instruction::STORE {
            rel: ValueRel::Local,
            index: 0,
        }, // store `3` in local variable at index 0
        Instruction::CONST { value: 4 },
        Instruction::STORE {
            rel: ValueRel::Local,
            index: 1,
        }, // store `4` in local variable at index 1
    ]);

    // Arguments assignment
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::BEGIN { args: 2, locals: 2 },
        Instruction::CONST { value: 3 },
        Instruction::STORE {
            rel: ValueRel::Arg,
            index: 0,
        }, // store `3` in function argument at index 0
        Instruction::CONST { value: 4 },
        Instruction::STORE {
            rel: ValueRel::Arg,
            index: 1,
        }, // store `4` in function argument at index 1
    ]);

    struct ExpectValue {
        metadata: FrameMetadata,
        args: Vec<Object>,
        locals: Vec<Object>,
    }

    let expected_results = vec![
        ExpectValue {
            metadata: FrameMetadata::new(2, 2, 0, 2),
            args: vec![Object::new_boxed(2), Object::new_boxed(2)],
            // un-initialized locals
            locals: vec![Object::new_boxed(0), Object::new_boxed(0)],
        },
        ExpectValue {
            metadata: FrameMetadata::new(2, 2, 0, 2),
            args: vec![Object::new_boxed(2), Object::new_boxed(2)],
            locals: vec![Object::new_boxed(3), Object::new_boxed(4)],
        },
        ExpectValue {
            metadata: FrameMetadata::new(2, 2, 0, 2),
            args: vec![Object::new_boxed(3), Object::new_boxed(4)],
            locals: vec![Object::new_boxed(0), Object::new_boxed(0)],
        },
    ];

    assert_eq!(programs.len(), expected_results.len());

    for (program, expected) in programs.into_iter().zip(expected_results) {
        let mut interp =
            Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, false));

        interp.run_on_program(program)?;

        let frame = FrameMetadata::get_from_stack(&interp.operand_stack, interp.frame_pointer)
            .ok_or(InterpreterError::StackUnderflow)?;

        assert_eq!(frame, expected.metadata);
        assert_eq!(
            frame
                .get_local_at(&interp.operand_stack, interp.frame_pointer, 0)
                .unwrap()
                .unwrap(),
            expected.locals[0].unwrap()
        );
        assert_eq!(
            frame
                .get_local_at(&interp.operand_stack, interp.frame_pointer, 1)
                .unwrap()
                .unwrap(),
            expected.locals[1].unwrap()
        );
        assert_eq!(
            frame
                .get_arg_at(&interp.operand_stack, interp.frame_pointer, 0)
                .unwrap()
                .unwrap(),
            expected.args[0].unwrap()
        );
        assert_eq!(
            frame
                .get_arg_at(&interp.operand_stack, interp.frame_pointer, 1)
                .unwrap()
                .unwrap(),
            expected.args[1].unwrap()
        );
    }

    Ok(())
}

#[test]
fn test_invalid_args_and_locals_assignment() -> Result<(), Box<dyn std::error::Error>> {
    let mut programs = Vec::new();

    // Local variables assignment
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 }, // return ip
        Instruction::BEGIN { args: 2, locals: 2 },
        Instruction::CONST { value: 3 },
        Instruction::STORE {
            rel: ValueRel::Local,
            index: 3,
        }, // invalid store `3` in local variable at index 3
    ]);

    // Arguments assignment
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::BEGIN { args: 2, locals: 2 },
        Instruction::CONST { value: 3 },
        Instruction::STORE {
            rel: ValueRel::Arg,
            index: 3,
        }, // invalid store `3` in function argument at index 0
    ]);

    let expected_results = vec![
        Err(InterpreterError::InvalidStoreIndex(ValueRel::Local, 3, 2)),
        Err(InterpreterError::InvalidStoreIndex(ValueRel::Arg, 3, 2)),
    ];

    assert_eq!(programs.len(), expected_results.len());

    for (program, expected) in programs.into_iter().zip(expected_results) {
        let mut interp =
            Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, false));

        let result = interp.run_on_program(program);

        assert!(result.is_err());
        assert_eq!(result, expected);
    }

    Ok(())
}

#[test]
fn test_arg_and_local_load() -> Result<(), InterpreterError> {
    let mut programs = Vec::new();

    // Loading uninitialized local variable
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 }, // return ip
        Instruction::BEGIN { args: 2, locals: 2 },
        Instruction::LOAD {
            rel: ValueRel::Local,
            index: 0,
        }, // load local variable at index 0
    ]);

    // Arguments load
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 3 },
        Instruction::CONST { value: 4 },
        Instruction::CONST { value: 5 },
        Instruction::CONST { value: 2 }, // ip
        Instruction::BEGIN { args: 4, locals: 2 },
        Instruction::LOAD {
            rel: ValueRel::Arg,
            index: 3,
        }, // load function argument at index 3
    ]);

    // Load mutated argument
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 }, // ip
        Instruction::BEGIN { args: 4, locals: 2 },
        Instruction::CONST { value: 3 },
        Instruction::STORE {
            rel: ValueRel::Arg,
            index: 0,
        }, // store `3` in function argument at index 0
        Instruction::LOAD {
            rel: ValueRel::Arg,
            index: 0,
        }, // load function argument at index 0
    ]);

    // Load mutated local
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 2 },
        Instruction::BEGIN { args: 2, locals: 2 },
        Instruction::CONST { value: 3 },
        Instruction::STORE {
            rel: ValueRel::Local,
            index: 0,
        }, // invalid store `3` in function argument at index 0
        Instruction::LOAD {
            rel: ValueRel::Local,
            index: 0,
        }, // load function argument at index 0
    ]);

    let expected_results = vec![0, 5, 3, 3];

    assert_eq!(programs.len(), expected_results.len());

    for (program, expected) in programs.into_iter().zip(expected_results) {
        let mut interp =
            Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, false));

        interp.run_on_program(program)?;

        let obj = interp.pop()?;

        assert_eq!(obj.unwrap(), expected);
    }

    Ok(())
}

#[test]
fn test_drop() -> Result<(), InterpreterError> {
    let mut programs = Vec::new();

    // Loading uninitialized local variable
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 3 },
        Instruction::DROP,
    ]);

    let expected_results = vec![2];

    assert_eq!(programs.len(), expected_results.len());

    for (program, expected) in programs.into_iter().zip(expected_results) {
        let mut interp =
            Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, false));

        interp.run_on_program(program)?;

        let obj = interp.pop()?;

        assert_eq!(obj.unwrap(), expected);
    }

    Ok(())
}

#[test]
fn test_dup() -> Result<(), InterpreterError> {
    let mut programs = Vec::new();

    // Loading uninitialized local variable
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 3 },
        Instruction::DUP,
    ]);

    let expected_results = vec![(3, 3)];

    assert_eq!(programs.len(), expected_results.len());

    for (program, expected) in programs.into_iter().zip(expected_results) {
        let mut interp =
            Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, false));

        interp.run_on_program(program)?;

        let (obj1, obj2) = (interp.pop()?, interp.pop()?);

        assert_eq!(obj1.unwrap(), expected.0);
        assert_eq!(obj2.unwrap(), expected.1);
    }

    Ok(())
}

#[test]
fn test_swap() -> Result<(), InterpreterError> {
    let mut programs = Vec::new();

    // Loading uninitialized local variable
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 3 },
        Instruction::SWAP,
    ]);

    let expected_results = vec![(2, 3)];

    assert_eq!(programs.len(), expected_results.len());

    for (program, expected) in programs.into_iter().zip(expected_results) {
        let mut interp =
            Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, false));

        interp.run_on_program(program)?;

        let (obj1, obj2) = (interp.pop()?, interp.pop()?);

        assert_eq!(obj1.unwrap(), expected.0);
        assert_eq!(obj2.unwrap(), expected.1);
    }

    Ok(())
}

#[test]
fn test_array() -> Result<(), Box<dyn std::error::Error>> {
    let mut programs = Vec::new();

    // 0 length array
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 3 },
        Instruction::CALL {
            offset: None,
            n: Some(0),
            name: Some(Builtin::Barray),
            builtin: true,
        },
        Instruction::DUP,
        Instruction::CALL {
            offset: None,
            n: None,
            name: Some(Builtin::Llength),
            builtin: true,
        },
    ]);

    // 2 length array
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 3 },
        Instruction::CALL {
            offset: None,
            n: Some(2),
            name: Some(Builtin::Barray),
            builtin: true,
        },
        Instruction::DUP,
        Instruction::CALL {
            offset: None,
            n: None,
            name: Some(Builtin::Llength),
            builtin: true,
        },
    ]);

    // TODO: Array of different types of elements (this is allowed in Lama)
    // programs.push(vec![
    //     Instruction::CONST { value: 1 },
    //     Instruction::CONST { value: 2 },
    //     Instruction::STRING { index: 0 }, // "main"
    //     Instruction::CALL {
    //         offset: None,
    //         n: Some(3),
    //         name: Some(Builtin::Barray),
    //         builtin: true,
    //     },
    //     Instruction::DUP,
    //     Instruction::CALL {
    //         offset: None,
    //         n: None,
    //         name: Some(Builtin::Llength),
    //         builtin: true,
    //     },
    // ]);

    let c_string = CString::new("main")?;
    let raw_ptr = c_string.as_ptr();
    let object = Object::try_from(raw_ptr).unwrap();

    let expected_results = vec![vec![], vec![2, 3]];

    assert_eq!(programs.len(), expected_results.len());

    for (program, expected) in programs.into_iter().zip(expected_results) {
        let mut interp = Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));

        interp.bf.put_string(CString::new("main")?);

        interp.run_on_program(program)?;

        let (len, obj) = (interp.pop()?, interp.pop()?);

        unsafe {
            assert_eq!(len.unwrap() as usize, expected.len());
            for (i, &value) in expected.iter().enumerate() {
                assert_eq!(
                    rtUnbox(get_array_el(
                        &*rtToData(
                            obj.as_ptr_mut()
                                .ok_or(InterpreterError::InvalidLengthForArray)?
                        ),
                        i
                    )),
                    value
                );
            }
        }
    }

    Ok(())
}

// Note: no Lwrite/Lread testing
#[test]
fn test_builtin_functions() -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        __gc_init();
    }

    let mut programs = Vec::new();

    // Llength of SEXP
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 3 },
        Instruction::CONST { value: 1 },
        // Nil()
        Instruction::SEXP {
            s_index: 5,
            n_members: 0,
        },
        // Cons(1, Nil())
        Instruction::SEXP {
            s_index: 0,
            n_members: 2,
        },
        // Cons(3, Cons(1, Nil()))
        Instruction::SEXP {
            s_index: 0,
            n_members: 2,
        },
        Instruction::DUP,
        Instruction::CALL {
            offset: None,
            n: None,
            name: Some(Builtin::Llength),
            builtin: true,
        },
    ]);

    // Llength of string
    programs.push(vec![
        Instruction::STRING {
            index: 0, // "Cons"
        },
        Instruction::CALL {
            offset: None,
            n: None,
            name: Some(Builtin::Llength),
            builtin: true,
        },
    ]);

    // TODO: Llength of closure

    let expected_results = vec![2, 4];

    assert_eq!(programs.len(), expected_results.len());

    for (program, expected) in programs.into_iter().zip(expected_results) {
        let mut interp = Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));

        interp.bf.put_string(CString::new("Cons")?);
        interp.bf.put_string(CString::new("Nil")?);

        interp.run_on_program(program)?;

        let obj = interp.pop()?;

        assert_eq!(obj.unwrap(), expected);
    }

    Ok(())
}

#[test]
fn test_conditional_jump() -> Result<(), Box<dyn std::error::Error>> {
    let mut programs = Vec::new();

    programs.push(vec![
        Instruction::CONST { value: 1 },
        Instruction::CJMP {
            offset: 10,
            kind: CompareJumpKind::ISNONZERO,
        },
        Instruction::CONST { value: 1 },
    ]);

    programs.push(vec![
        Instruction::CONST { value: 0 },
        Instruction::CJMP {
            offset: 10,
            kind: CompareJumpKind::ISNONZERO,
        },
        Instruction::CONST { value: 1 },
    ]);

    programs.push(vec![
        Instruction::CONST { value: 1 },
        Instruction::CJMP {
            offset: 10,
            kind: CompareJumpKind::ISZERO,
        },
        Instruction::CONST { value: 1 },
    ]);

    programs.push(vec![
        Instruction::CONST { value: 0 },
        Instruction::CJMP {
            offset: 10,
            kind: CompareJumpKind::ISZERO,
        },
        Instruction::CONST { value: 1 },
    ]);

    // expected ip
    let expected_results = vec![10, 0, 0, 10];

    assert_eq!(programs.len(), expected_results.len());

    for (program, expected) in programs.into_iter().zip(expected_results) {
        let mut interp = Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));
        interp.run_on_program(program)?;

        let ip = interp.ip;

        assert_eq!(ip, expected);
    }

    Ok(())
}

#[test]
fn test_elem() -> Result<(), Box<dyn std::error::Error>> {
    let mut programs = Vec::new();

    // 2 length array
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 3 },
        Instruction::CALL {
            offset: None,
            n: Some(2),
            name: Some(Builtin::Barray),
            builtin: true,
        },
        Instruction::DUP,
        Instruction::CONST { value: 1 }, // array[1] => 3
        Instruction::ELEM,
    ]);

    // 2 length array
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 3 },
        Instruction::CALL {
            offset: None,
            n: Some(2),
            name: Some(Builtin::Barray),
            builtin: true,
        },
        Instruction::DUP,
        Instruction::CONST { value: 0 }, // array[0] => 2
        Instruction::ELEM,
    ]);

    // string
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 3 },
        Instruction::STRING { index: 0 }, // "main"
        Instruction::DUP,
        Instruction::CONST { value: 0 }, // 'm'
        Instruction::ELEM,
    ]);

    let expected_results = vec![3, 2, 109];

    assert_eq!(programs.len(), expected_results.len());

    for (program, expected) in programs.into_iter().zip(expected_results) {
        let mut interp = Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));

        interp.bf.put_string(CString::new("main")?);

        interp.run_on_program(program)?;

        let obj = interp.pop()?;
        assert_eq!(obj.unwrap(), expected);
    }

    Ok(())
}

#[test]
fn test_invalid_elem() -> Result<(), Box<dyn std::error::Error>> {
    let mut programs = Vec::new();

    // 0 length array
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 3 },
        Instruction::CALL {
            offset: None,
            n: Some(0),
            name: Some(Builtin::Barray),
            builtin: true,
        },
        Instruction::DUP,
        Instruction::CONST { value: 1 },
        Instruction::ELEM,
    ]);

    // 2 length array
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 3 },
        Instruction::CALL {
            offset: None,
            n: Some(2),
            name: Some(Builtin::Barray),
            builtin: true,
        },
        Instruction::DUP,
        Instruction::CONST { value: 3 }, // array[3]
        Instruction::ELEM,
    ]);

    let expected_results = vec![
        InterpreterError::OutOfBoundsAccess(1, 0),
        InterpreterError::OutOfBoundsAccess(3, 2),
    ];

    assert_eq!(programs.len(), expected_results.len());

    for (program, expected) in programs.into_iter().zip(expected_results) {
        let mut interp = Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));

        interp.bf.put_string(CString::new("main")?);

        let result = interp.run_on_program(program);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), expected);
    }

    Ok(())
}

#[test]
fn test_array_tag() -> Result<(), Box<dyn std::error::Error>> {
    let mut programs = Vec::new();

    // 0 length array
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 3 },
        Instruction::CALL {
            offset: None,
            n: Some(0),
            name: Some(Builtin::Barray),
            builtin: true,
        },
        Instruction::DUP,
        Instruction::ARRAY { n: 0 }, // => 1
    ]);

    // 2 length array
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 3 },
        Instruction::CALL {
            offset: None,
            n: Some(2),
            name: Some(Builtin::Barray),
            builtin: true,
        },
        Instruction::DUP,
        Instruction::ARRAY { n: 2 }, // => 1
    ]);

    // 2 length array
    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 3 },
        Instruction::CALL {
            offset: None,
            n: Some(2),
            name: Some(Builtin::Barray),
            builtin: true,
        },
        Instruction::DUP,
        Instruction::ARRAY { n: 3 }, // => 0
    ]);

    programs.push(vec![
        Instruction::CONST { value: 2 },
        Instruction::CONST { value: 3 },
        Instruction::ARRAY { n: 0 }, // => 0
    ]);

    let expected_results = vec![1, 1, 0, 0];

    assert_eq!(programs.len(), expected_results.len());

    for (program, expected) in programs.into_iter().zip(expected_results) {
        let mut interp = Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));

        interp.bf.put_string(CString::new("main")?);

        interp.run_on_program(program)?;

        let obj = interp.pop()?;

        assert_eq!(obj.unwrap(), expected);
    }

    Ok(())
}
