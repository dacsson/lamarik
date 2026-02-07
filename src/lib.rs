#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::{ffi::CString, os::raw::c_void};

pub mod bytecode;
pub mod disasm;
mod frame;
pub mod interpreter;
mod numeric;
pub mod object;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

/// Create a new S-expression with the given tag and arguments.
/// Returns a pointer to *contents* of the S-expression.
/// To retrieve the actual S-expression, use `rtToSexp`.
fn new_sexp(tag: CString, mut args: Vec<i64>) -> *mut c_void {
    unsafe {
        let tag_hash = LtagHash(tag.into_raw());

        let mut contents = vec![tag_hash];
        contents.resize(args.len() + 1, 0);

        contents.append(&mut args);

        Bsexp(
            contents.as_mut_ptr(),        /* [args_1,...,arg_n, tag] */
            rtBox(args.len() as i64) + 1, /* n args */
        )
    }
}

fn get_sexp_el(sexp: &sexp, index: usize) -> i64 {
    unsafe { (sexp.contents.as_ptr() as *const i64).add(index).read() }
}

impl PartialEq for sexp {
    fn eq(&self, other: &Self) -> bool {
        unsafe {
            let self_header = self.data_header;
            let other_header = other.data_header;

            let self_len = rtLen(self_header);
            let other_len = rtLen(other_header);

            let tag_and_len_eq = self.tag == other.tag && self_len == other_len;

            if !tag_and_len_eq {
                return false;
            }

            let hashed_cons_string = LtagHash(CString::new("cons").unwrap().into_raw());
            if self.tag == hashed_cons_string.try_into().unwrap() {
                for i in 0..self_len {
                    let el = get_sexp_el(self, i as usize);
                    let other_el = get_sexp_el(other, i as usize);

                    if isUnboxed(el) {
                        if el != other_el {
                            return false;
                        }
                    } else {
                        return rtToSexp(el as *mut c_void) == rtToSexp(other_el as *mut c_void);
                    }
                }
            } else {
                // TODO: compare contents of arbitrary types
                //       we need PartialEq for all types
                //       look at runtime.c: static void printValue (void *p)
                return true;
            }

            false
        }
    }
}

/// Given a c_void pointer to arbitrary data (of Lama aggregate type),
/// returns the tag of the data.
fn get_data_tag(ptr: *mut c_void) -> i32 {
    unsafe {
        let data = rtToData(ptr) as *const data;
        let header = (*data).data_header;
        rtTag(header as u64)
    }
}

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
