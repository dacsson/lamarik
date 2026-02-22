#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::{ffi::CString, os::raw::c_void};

pub mod analyzer;
pub mod bytecode;
pub mod disasm;
mod frame;
pub mod interpreter;
mod numeric;
pub mod object;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

const CONS_TAG_HASH: i64 = 1697575;
const NIL_TAG_HASH: i64 = 115865;

// #define UNBOX(x) (((aint)(x)) >> 1)
#[inline(always)]
fn rtUnbox(x: i64) -> i64 {
    (((x as i64) >> 1) as i64)
}

// #define BOX(x) ((((aint)(x)) << 1) | 1)
#[inline(always)]
fn rtBox(x: i64) -> i64 {
    ((((x as i64) << 1) | 1) as i64)
}

// #define UNBOXED(x) (((aint)(x)) & 1)
#[inline(always)]
fn isUnboxed(x: i64) -> bool {
    (((x as i64) & 1) == 1)
}

// #  define DATA_HEADER_SZ (sizeof(auint) + sizeof(ptrt))
const DATA_HEADER_SZ: usize = std::mem::size_of::<auint>() + std::mem::size_of::<ptrt>();

// define TO_DATA(x) ((data *)((char *)(x)-DATA_HEADER_SZ))
#[inline(always)]
fn rtToData(x: *mut c_void) -> *mut data {
    unsafe { (x as *mut u8).offset(-(DATA_HEADER_SZ as isize)) as *mut data }
}

// #define TO_SEXP(x) ((sexp *)((char *)(x)-DATA_HEADER_SZ))
#[inline(always)]
fn rtToSexp(x: *mut c_void) -> *mut sexp {
    unsafe { (x as *mut u8).offset(-(DATA_HEADER_SZ as isize)) as *mut sexp }
}

// #define LEN_MASK (UINT64_MAX^7)
const LEN_MASK: u64 = u64::MAX ^ 7;

// #define LEN(x) (ptrt)(((ptrt)x & LEN_MASK) >> 3)
#[inline(always)]
fn rtLen(x: u64) -> ptrt {
    (((x & LEN_MASK) >> 3) as ptrt)
}

// #define TAG(x) (x & 7)
#[inline(always)]
fn rtTag(x: u64) -> i32 {
    (x & 7) as i32
}

/// Create a new S-expression with the given tag and arguments.
/// Returns a pointer to *contents* of the S-expression.
/// To retrieve the actual S-expression, use `rtToSexp`.
#[inline(always)]
fn new_sexp(tag: CString, mut args: Vec<i64>) -> *mut c_void {
    unsafe {
        let tag_hash = if tag.to_bytes() == "cons".as_bytes() {
            CONS_TAG_HASH
        } else if tag.to_bytes() == "nil".as_bytes() {
            NIL_TAG_HASH
        } else {
            LtagHash(tag.into_raw())
        };

        args.push(tag_hash);

        Bsexp(
            args.as_mut_ptr(),        /* [args_1,...,arg_n, tag] */
            rtBox(args.len() as i64), /* n args */
        )
    }
}

/// Create a new lama string.
#[inline(always)]
fn new_string(bytes: Vec<u8>) -> Result<*mut c_void, Box<dyn std::error::Error>> {
    unsafe {
        let c_string = CString::from_vec_with_nul(bytes)?;
        let as_ptr = c_string.into_raw();

        let mut slice = vec![as_ptr as i64];

        Ok(Bstring(slice.as_mut_ptr()))
    }
}

/// Create array from given elements.
/// Returns a pointer to *contents* of the array.
/// To retrieve the actual array, use `rtToData`.
#[inline(always)]
fn new_array(mut elements: Vec<i64>) -> *mut c_void {
    unsafe {
        Barray(
            elements.as_mut_ptr(),        /* [args_1,...,arg_n, tag] */
            rtBox(elements.len() as i64), /* n args */
        )
    }
}

/// Remember that arrays store raw values, meaning callee is responsible for unboxing them.
#[inline(always)]
fn get_array_el(arr: &data, index: usize) -> i64 {
    unsafe { (arr.contents.as_ptr() as *const i64).add(index).read() }
}

/// Create a new closure object
/// Returns a pointer to *contents* of the closure.
/// To retrieve the actual closure, use `rtToData`.
#[inline(always)]
fn new_closure(mut args: Vec<i64>) -> *mut c_void {
    unsafe {
        Bclosure(
            args.as_mut_ptr(),        /* [args_1,...,arg_n, tag] */
            rtBox(args.len() as i64), /* n args */
        )
    }
}

#[inline(always)]
fn get_captured_variable(closure: &data, index: usize) -> i64 {
    unsafe {
        // index + 1 because the first element is the offset
        (closure.contents.as_ptr() as *const i64)
            .add(index + 1)
            .read()
    }
}

#[inline(always)]
fn set_captured_variable(closure: &mut data, index: usize, value: i64) {
    unsafe {
        // index + 1 because the first element is the offset
        (closure.contents.as_ptr() as *mut i64)
            .add(index + 1)
            .write(value);
    }
}

/// Callee is responsible for ensuring that index is within bounds.
#[inline(always)]
fn set_array_el(arr: &mut data, index: usize, value: i64) {
    unsafe {
        (arr.contents.as_ptr() as *mut i64).add(index).write(value);
    }
}

/// Remember that S-expressions store raw values, meaning callee is responsible for unboxing them.
#[inline(always)]
fn get_sexp_el(sexp: &sexp, index: usize) -> i64 {
    unsafe { (sexp.contents.as_ptr() as *const i64).add(index).read() }
}

/// Callee is responsible for ensuring that index is within bounds.
#[inline(always)]
fn set_sexp_el(sexp: &mut sexp, index: usize, value: i64) {
    unsafe {
        (sexp.contents.as_ptr() as *mut i64).add(index).write(value);
    }
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

impl PartialEq for data {
    fn eq(&self, other: &Self) -> bool {
        unsafe {
            let self_header = self.data_header;
            let other_header = other.data_header;

            let self_tag = rtTag(self_header);
            let other_tag = rtTag(other_header);

            if self_tag != other_tag {
                return false;
            }

            match self_tag as u32 {
                STRING_TAG => strcmp(self.contents.as_ptr(), other.contents.as_ptr()) == 0,
                // SEXP_TAG => {
                //     let self_sexp = self as &sexp;
                //     let other_sexp = rtToSexp(other_header as *mut c_void);
                //     self_sexp == other_sexp
                // }
                ARRAY_TAG => {
                    let self_len = rtLen(self_header);
                    let other_len = rtLen(other_header);
                    if self_len != other_len {
                        return false;
                    }
                    for i in 0..self_len {
                        let self_el = get_array_el(self, i as usize);
                        let other_el = get_array_el(other, i as usize);

                        let is_unboxed_self_el = isUnboxed(self_el);
                        let is_unboxed_other_el = isUnboxed(other_el);

                        if is_unboxed_self_el != is_unboxed_other_el {
                            return false;
                        }

                        if is_unboxed_self_el {
                            if self_el != other_el {
                                return false;
                            }
                        } else {
                            let self_el_as_ptr = self_el as *mut c_void;
                            let other_el_as_ptr = other_el as *mut c_void;

                            return *rtToData(self_el_as_ptr) == *rtToData(other_el_as_ptr);
                        }
                    }

                    true
                }
                _ => panic!("Unsupported type for equality comparison"),
            }
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
