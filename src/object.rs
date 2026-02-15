//! Interpreter Objet type description

use crate::{get_obj_header_ptr, get_type_header_ptr, isUnboxed, lama_type, rtBox, rtUnbox};
use std::fmt::{Debug, Display, Formatter};
use std::os::raw::c_void;

/// An element of operand stack in interpreter:
/// - Pointers (should be) are boxed due to alignment
/// - Other objects get boxed on creation and unboxed on usage
#[derive(Debug, Clone)]
pub enum Object {
    Boxed(i64),
    Unboxed(i64),
}

#[derive(Debug, PartialEq)]
pub enum ObjectError {
    CreatingUnboxedObjectWithBoxedValue,
    CreatingBoxedObjectWithAlreadyBoxedValue,
}

impl Display for ObjectError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ObjectError::CreatingUnboxedObjectWithBoxedValue => {
                write!(f, "Creating unboxed object with boxed value")
            }
            ObjectError::CreatingBoxedObjectWithAlreadyBoxedValue => {
                write!(f, "Creating boxed object with already boxed value")
            }
        }
    }
}

impl std::error::Error for ObjectError {}

impl Object {
    pub fn new_boxed(value: i64) -> Self {
        unsafe { Object::Boxed(rtBox(value)) }
    }

    pub fn new_unboxed(value: i64) -> Self {
        Object::Unboxed(value)
    }

    /// Creates a new empty unboxed object with a default value of 0
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

    /// Data is stored as raw i64, hence we need to translate back to pointer
    /// iff it was created from pointer
    pub fn as_ptr<T>(&self) -> Option<*const T> {
        match self {
            Object::Boxed(v) => Some(*v as *const T),
            Object::Unboxed(_) => None,
        }
    }

    /// [`as_ptr`]
    pub fn as_ptr_mut<T>(&self) -> Option<*mut T> {
        match self {
            Object::Boxed(v) => Some(*v as *mut T),
            Object::Unboxed(_) => None,
        }
    }

    /// Get lama type of object
    pub fn lama_type(&mut self) -> Option<lama_type> {
        unsafe {
            if let Some(as_ptr) = self.as_ptr_mut::<c_void>() {
                let header_ptr = get_obj_header_ptr(as_ptr);
                Some(get_type_header_ptr(header_ptr))
            } else {
                None
            }
        }
    }
}

/// Create unboxed object from string pointer.
/// Pointers come boxed by default due to alignment requirements.
impl TryFrom<*const i8> for Object {
    type Error = ();

    fn try_from(ptr: *const i8) -> Result<Self, Self::Error> {
        Ok(Object::Boxed(ptr.addr() as i64))
    }
}

/// Create unboxed object from pointer to aggreggate type contents.
/// Pointers come boxed by default due to alignment requirements.
impl TryFrom<*mut c_void> for Object {
    type Error = ();

    fn try_from(ptr: *mut c_void) -> Result<Self, Self::Error> {
        Ok(Object::Boxed(ptr.addr() as i64))
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
    use crate::rtToData;
    use std::ffi::{CStr, CString};

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

    #[test]
    fn test_create_from_string() -> Result<(), Box<dyn std::error::Error>> {
        let c_string = CString::new("main")?;
        let raw_ptr: *const i8 = c_string.into_raw();
        let obj = Object::try_from(raw_ptr).map_err(|_| "Error at Object::try_from(raw_ptr)")?;

        unsafe {
            let as_ptr = obj.as_ptr_mut().ok_or("Failed to get pointer")?;
            let contents = (*rtToData(as_ptr)).contents.as_ptr();
            let c_string_again = CStr::from_ptr(contents);

            assert_eq!(*c_string_again, CString::new("main")?);
        }

        Ok(())
    }
}
