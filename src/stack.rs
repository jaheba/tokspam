//! Fallible segmented stacks. Suspended scans retain entries below `base`.
use std::ops::{Deref, DerefMut};

/// Indices survive relocation when a growing vector reserves more space.
#[derive(Clone, Copy, Default)]
pub(crate) struct Mark {
    base: usize,
    len: usize,
}

pub(crate) struct Stack<T> {
    data: Vec<T>,
    base: usize,
    capacity_limit: usize,
    failed: bool,
}

impl<T> Default for Stack<T> {
    fn default() -> Self {
        Self::with_storage(0, false)
    }
}

impl<T> Stack<T> {
    pub(crate) fn with_storage(capacity: usize, fixed: bool) -> Self {
        let mut data = Vec::new();
        let failed = fixed && data.try_reserve_exact(capacity).is_err();
        Self {
            data,
            base: 0,
            capacity_limit: if fixed { capacity } else { usize::MAX },
            failed,
        }
    }

    pub(crate) fn failed(&self) -> bool {
        self.failed
    }

    pub(crate) fn reset(&mut self) {
        self.data.clear();
        self.base = 0;
        self.failed = false;
    }

    pub(crate) fn clear(&mut self) {
        self.data.truncate(self.base);
    }

    #[cfg(feature = "regions")]
    pub(crate) fn truncate(&mut self, length: usize) {
        self.data.truncate(self.base + length);
    }

    pub(crate) fn is_full(&self) -> bool {
        self.data.len() == self.capacity_limit
    }

    // A failed push never changes the segment. Callers stop the scan before
    // relying on the missing entry; allocation failure is an incomplete result.
    pub(crate) fn push(&mut self, value: T) -> bool {
        if self.data.len() == self.data.capacity() && !self.grow() {
            self.failed = true;
            return false;
        }
        if self.is_full() {
            self.failed = true;
            return false;
        }
        self.data.push(value);
        true
    }

    #[cold]
    fn grow(&mut self) -> bool {
        if self.is_full() {
            return false;
        }
        let target = self
            .data
            .capacity()
            .saturating_mul(2)
            .max(4)
            .min(self.capacity_limit);
        self.data
            .try_reserve_exact(target - self.data.len())
            .is_ok()
    }

    pub(crate) fn suspend(&mut self) -> Mark {
        let mark = Mark {
            base: self.base,
            len: self.data.len(),
        };
        self.base = self.data.len();
        mark
    }

    pub(crate) fn restore(&mut self, mark: Mark) {
        self.base = mark.base;
        self.data.truncate(mark.len);
    }

    pub(crate) fn pop(&mut self) -> Option<T> {
        if self.data.len() == self.base {
            None
        } else {
            self.data.pop()
        }
    }
}

impl<T> Deref for Stack<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.data[self.base..]
    }
}
impl<T> DerefMut for Stack<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.data[self.base..]
    }
}
