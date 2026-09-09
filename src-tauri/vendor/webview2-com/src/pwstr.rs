use std::{fmt::Display, marker::PhantomData, mem, ptr};

use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{Globalization::lstrlenW, System::Com},
};

/// RAII holder for a [`PWSTR`] which is allocated with [`Com::CoTaskMemAlloc`] and freed
/// with [`Com::CoTaskMemFree`] when dropped.
pub struct CoTaskMemPWSTR<'a>(PWSTR, PhantomData<&'a PWSTR>);

/// Constant guard object tied to the lifetime of the [`CoTaskMemPWSTR`] so that it
/// is safe to dereference the [`PCWSTR`] as long as both are still in scope.
pub struct CoTaskMemRef<'a>(PCWSTR, PhantomData<&'a PCWSTR>);

impl CoTaskMemRef<'_> {
    pub fn as_pcwstr(&self) -> &PCWSTR {
        &self.0
    }
}

impl<'a> From<&'a CoTaskMemPWSTR<'a>> for CoTaskMemRef<'a> {
    fn from(value: &'a CoTaskMemPWSTR<'a>) -> Self {
        Self(PCWSTR::from_raw(value.0.as_ptr()), PhantomData)
    }
}

/// Mutable guard object tied to the lifetime of the [`CoTaskMemPWSTR`] so that it
/// is safe to dereference the [`PWSTR`] as long as both are still in scope.
pub struct CoTaskMemMut<'a>(&'a PWSTR);

impl<'a> CoTaskMemMut<'a> {
    pub fn as_pwstr(&mut self) -> &'a PWSTR {
        self.0
    }
}

impl<'a> From<&'a mut CoTaskMemPWSTR<'a>> for CoTaskMemMut<'a> {
    fn from(value: &'a mut CoTaskMemPWSTR<'a>) -> Self {
        Self(&value.0)
    }
}

impl<'a> CoTaskMemPWSTR<'a> {
    /// Get a mutable [`PWSTR`] guard which borrows the pointer.
    pub fn as_mut(&'a mut self) -> CoTaskMemMut<'a> {
        From::from(self)
    }

    /// Get a constant [`PCWSTR`] guard which borrows the pointer.
    pub fn as_ref(&'a self) -> CoTaskMemRef<'a> {
        From::from(self)
    }

    /// Take the [`PWSTR`] pointer and hand off ownership so that it is not freed when the `CoTaskMemPWSTR` is dropped.
    pub fn take(&mut self) -> PWSTR {
        let result = self.0;
        self.0 = PWSTR::null();
        result
    }
}

impl Drop for CoTaskMemPWSTR<'_> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                Com::CoTaskMemFree(Some(self.0.as_ptr() as *mut _ as *const _));
            }
        }
    }
}

impl Default for CoTaskMemPWSTR<'_> {
    fn default() -> Self {
        Self(PWSTR::null(), PhantomData)
    }
}

impl From<PWSTR> for CoTaskMemPWSTR<'_> {
    fn from(value: PWSTR) -> Self {
        Self(value, PhantomData)
    }
}

impl From<&str> for CoTaskMemPWSTR<'_> {
    fn from(value: &str) -> Self {
        match value {
            "" => Default::default(),
            value => {
                let encoded: Vec<_> = value.encode_utf16().chain(std::iter::once(0)).collect();

                unsafe {
                    let mut buffer =
                        Com::CoTaskMemAlloc(encoded.len() * mem::size_of::<u16>()) as *mut u16;
                    let result = PWSTR::from_raw(buffer);

                    for char in encoded {
                        *buffer = char;
                        buffer = buffer.add(1);
                    }

                    Self(result, PhantomData)
                }
            }
        }
    }
}

impl Display for CoTaskMemPWSTR<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = string_from_pcwstr(self.as_ref().as_pcwstr());
        f.write_str(value.as_str())
    }
}

/// Copy a [`PCWSTR`] from an input param to a [`String`].
pub fn string_from_pcwstr(source: &PCWSTR) -> String {
    if source.0.is_null() {
        String::new()
    } else {
        let len = unsafe { lstrlenW(*source) };

        if len > 0 {
            unsafe {
                let buffer = ptr::slice_from_raw_parts(source.0, len as usize);
                String::from_utf16_lossy(&*buffer)
            }
        } else {
            String::new()
        }
    }
}

/// Copy a [`PWSTR`] allocated with [`Com::CoTaskMemAlloc`] from an input param to a [`String`]
/// and free the original buffer with [`Com::CoTaskMemFree`].
pub fn take_pwstr(source: PWSTR) -> String {
    CoTaskMemPWSTR::from(source).to_string()
}

/// Allocate a [`PWSTR`] with [`Com::CoTaskMemAlloc`] and copy a [`&str`] into it.
pub fn pwstr_from_str(source: &str) -> PWSTR {
    CoTaskMemPWSTR::from(source).take()
}
