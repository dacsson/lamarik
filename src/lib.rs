#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

pub mod bytecode;
pub mod disasm;
pub mod interpreter;
mod numeric;
pub mod object;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_link_smoke_test() {
        unsafe {
            assert_eq!(isUnboxed(0), false);
            assert_eq!(isUnboxed(1), true);
        }
    }
}
