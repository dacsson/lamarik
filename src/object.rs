//! Interpreter Objet type description

use crate::{get_obj_header_ptr, get_type_header_ptr, lama_type, rtBox, rtUnbox};
use std::fmt::{Debug, Display, Formatter};
use std::ptr;

/// An element of operand stack in interpreter:
/// - Pointers (should be) are boxed due to alignment
/// - Other objects get boxed on creation and unboxed on usage
pub enum Object {
    Boxed(i64),
    Unboxed(i64),
}

impl Object {
    pub fn new_boxed(value: i64) -> Self {
        unsafe { Object::Boxed(rtBox(value)) }
    }

    pub fn new_unboxed(value: i64) -> Self {
        Object::Unboxed(value)
    }

    pub fn new_empty() -> Self {
        Object::Unboxed(0)
    }

    /// Retrieve objects inner value
    pub fn unwrap(&self) -> i64 {
        match self {
            Object::Boxed(v) => unsafe { rtUnbox(*v) },
            Object::Unboxed(v) => *v,
        }
    }

    /// Data is stored as raw i64, hence we need to translate to pointer back
    /// if it was created from pointer
    pub fn as_mut_ptr(&mut self) -> Option<*mut i64> {
        match self {
            Object::Boxed(_) => None,
            Object::Unboxed(v) => Some(ptr::from_mut(v)),
        }
    }

    /// Get lama type of object
    fn lama_type(&mut self) -> Option<lama_type> {
        unsafe {
            if let Some(as_ptr) = self.as_mut_ptr() {
                let header_ptr = get_obj_header_ptr(as_ptr as *mut _);
                Some(get_type_header_ptr(header_ptr))
            } else {
                None
            }
        }
    }
}

impl Display for Object {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Object::Boxed(v) => write!(f, "Boxed({})", unsafe { rtUnbox(*v) }),
            Object::Unboxed(v) => write!(f, "Unboxed({})", *v),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test creation of objects and that runtime will
    /// detect them either boxed or unboxed properly
    #[test]
    fn test_creation() {
        let obj1 = Object::new_boxed(1);
        let obj2 = Object::new_unboxed(1);
        let obj3 = Object::new_empty();
        let obj4 = Object::new_boxed(2);

        unsafe {
            if let Object::Boxed(v) = obj1 {
                assert_eq!(v, rtBox(1));
            }
            assert_eq!(obj1.unwrap(), rtUnbox(rtBox(1)));
            if let Object::Unboxed(v) = obj2 {
                assert_eq!(v, 1);
            }
            assert_eq!(obj2.unwrap(), 1);
            assert_eq!(obj3.unwrap(), 0);
            if let Object::Boxed(v) = obj4 {
                assert_eq!(v, rtBox(2));
            }
            assert_eq!(obj4.unwrap(), rtUnbox(rtBox(2)));
        }
    }
}
