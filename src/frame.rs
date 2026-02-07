//! This module defines frame metadata for the interpreter.
//! Because we have only one stack, we keep index
//! of the frame pointer.

use crate::object::Object;

/// In operand stack out frame data looks like this:
/// ```txt
/// ... <- frame points to this index
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
    pub n_locals: i64,
    pub n_args: i64,
    pub ret_frame_pointer: usize,
    pub ret_ip: usize,
}

impl<'a> FrameMetadata {
    /// Given an operand stack, construct a new frame metadata.
    /// Accompanies the `BEGIN` instruction.
    pub fn get_from_stack(stack: &Vec<Object>, frame_pointer: usize) -> Option<FrameMetadata> {
        let n_args = stack.get(frame_pointer + 1)?.unwrap();
        let n_locals = stack.get(frame_pointer + 2)?.unwrap();
        let ret_frame_pointer = stack.get(frame_pointer + 3)?.unwrap() as usize;
        let ret_ip = stack.get(frame_pointer + 4)?.unwrap() as usize;

        Some(FrameMetadata {
            n_locals,
            n_args,
            ret_frame_pointer,
            ret_ip,
        })
    }

    pub fn get_arg_at(
        &'a self,
        stack: &'a Vec<Object>,
        frame_pointer: usize,
        index: usize,
    ) -> Option<&'a Object> {
        let arg_index = frame_pointer + 5 + index;
        stack.get(arg_index)
    }

    pub fn get_local_at(
        &'a self,
        stack: &'a Vec<Object>,
        frame_pointer: usize,
        index: usize,
    ) -> Option<&'a Object> {
        let local_index = frame_pointer + 5 + self.n_args as usize + index;
        stack.get(local_index)
    }

    pub fn set_local_at(
        &'a mut self,
        stack: &'a mut Vec<Object>,
        frame_pointer: usize,
        index: usize,
        value: Object,
    ) -> Result<(), String> {
        let local_index = frame_pointer + 5 + self.n_args as usize + index;

        if local_index >= stack.len() || index > self.n_locals as usize {
            return Err("Index out of bounds".into());
        }

        stack[local_index] = value;
        Ok(())
    }

    pub fn set_arg_at(
        &'a mut self,
        stack: &'a mut Vec<Object>,
        frame_pointer: usize,
        index: usize,
        value: Object,
    ) -> Result<(), String> {
        let arg_index = frame_pointer + 5 + index;

        if arg_index >= stack.len() || index > self.n_args as usize {
            return Err("Index out of bounds".into());
        }

        stack[arg_index] = value;
        Ok(())
    }

    #[cfg(test)]
    pub fn new(
        n_locals: i64,
        n_args: i64,
        ret_frame_pointer: usize,
        ret_ip: usize,
    ) -> FrameMetadata {
        FrameMetadata {
            n_locals,
            n_args,
            ret_frame_pointer,
            ret_ip,
        }
    }
}
