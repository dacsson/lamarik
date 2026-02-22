//! This module defines frame metadata for the interpreter.
//! Because we have only one stack, we keep index
//! of the frame pointer.

use crate::object::Object;

/// In operand stack out frame data looks like this:
/// ```txt
/// ... <- frame points to this index
/// CLOSURE_OBJ // possibly none
/// ARGS_COUNT
/// LOCALS_COUNT
/// OLD_FRAME_POINTER
/// OLD_IP
/// ARG1
/// ARG2
/// ...
/// ARGN
/// LOCAL1
/// LOCAL2
/// ...
/// LOCALN
/// ```
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct FrameMetadata {
    pub closure_obj: i64,
    pub n_locals: i64,
    pub n_args: i64,
    pub ret_frame_pointer: usize,
    pub ret_ip: usize,
}

impl<'a> FrameMetadata {
    /// Given an operand stack, construct a new frame metadata.
    /// Accompanies the `BEGIN` instruction.
    #[inline(always)]
    pub fn get_from_stack(stack: &[Object], frame_pointer: usize) -> Option<FrameMetadata> {
        let closure_obj = stack.get(frame_pointer + 1)?.raw();
        let n_args = stack.get(frame_pointer + 2)?.unwrap();
        let n_locals = stack.get(frame_pointer + 3)?.unwrap();
        let ret_frame_pointer = stack.get(frame_pointer + 4)?.unwrap() as usize;
        let ret_ip = stack.get(frame_pointer + 5)?.unwrap() as usize;

        Some(FrameMetadata {
            closure_obj,
            n_locals,
            n_args,
            ret_frame_pointer,
            ret_ip,
        })
    }

    #[inline(always)]
    pub fn get_arg_at(
        &'a self,
        stack: &'a [Object],
        frame_pointer: usize,
        index: usize,
    ) -> Option<&'a Object> {
        let arg_index = frame_pointer + 6 + index;
        stack.get(arg_index)
    }

    #[inline(always)]
    pub fn get_local_at(
        &'a self,
        stack: &'a [Object],
        frame_pointer: usize,
        index: usize,
    ) -> Option<&'a Object> {
        let local_index = frame_pointer + 6 + self.n_args as usize + index;
        stack.get(local_index)
    }

    #[inline(always)]
    pub fn set_local_at(
        &'a mut self,
        stack: &'a mut [Object],
        frame_pointer: usize,
        index: usize,
        value: Object,
    ) -> Result<(), String> {
        let local_index = frame_pointer + 6 + self.n_args as usize + index;

        #[cfg(feature = "runtime_checks")]
        if local_index >= stack.len() || index > self.n_locals as usize {
            return Err("Index out of bounds".into());
        }

        stack[local_index] = value;
        Ok(())
    }

    #[inline(always)]
    pub fn set_arg_at(
        &'a mut self,
        stack: &'a mut [Object],
        frame_pointer: usize,
        index: usize,
        value: Object,
    ) -> Result<(), String> {
        let arg_index = frame_pointer + 6 + index;

        #[cfg(feature = "runtime_checks")]
        if arg_index >= stack.len() || index > self.n_args as usize {
            return Err("Index out of bounds".into());
        }

        stack[arg_index] = value;
        Ok(())
    }

    pub fn save_closure(
        &'a mut self,
        stack: &'a mut [Object],
        frame_pointer: usize,
        closure_obj: Object,
    ) -> Result<(), String> {
        let closure_index = frame_pointer + 1;

        #[cfg(feature = "runtime_checks")]
        if closure_index >= stack.len() {
            return Err("Index out of bounds".into());
        }

        stack[closure_index] = closure_obj;
        Ok(())
    }

    #[inline(always)]
    pub fn get_closure(
        &'a mut self,
        stack: &'a mut Vec<Object>,
        frame_pointer: usize,
    ) -> Result<&'a Object, String> {
        let closure_index = frame_pointer + 1;

        #[cfg(feature = "runtime_checks")]
        if closure_index >= stack.len() {
            return Err("Index out of bounds".into());
        }

        Ok(&stack[closure_index])
    }

    #[cfg(test)]
    pub fn new(
        n_locals: i64,
        n_args: i64,
        ret_frame_pointer: usize,
        ret_ip: usize,
    ) -> FrameMetadata {
        FrameMetadata {
            closure_obj: 0,
            n_locals,
            n_args,
            ret_frame_pointer,
            ret_ip,
        }
    }
}
