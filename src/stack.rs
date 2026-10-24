//! Inline segmented stacks; annotation scans share buffers without copying entries.
use std::ops::{Deref, DerefMut};

pub(crate) const SCAN_FRAMES: usize = crate::MAX_ANNOTATION_DEPTH as usize + 1;
// Roots for all live scans plus one entry observed before structural rejection.
pub(crate) const STRUCTURAL_CAPACITY: usize = crate::MAX_NESTING_DEPTH as usize + SCAN_FRAMES + 1;

#[derive(Clone, Copy, Default)]
pub(crate) struct Mark {
    base: u16,
    len: u16,
}

pub(crate) struct Stack<T, const N: usize> {
    data: [T; N],
    base: usize,
    len: usize,
}

impl<T: Default, const N: usize> Default for Stack<T, N> {
    fn default() -> Self {
        // Every stack mark is an index into one bounded inline array.
        const {
            assert!(N <= u16::MAX as usize);
        }

        Self {
            data: std::array::from_fn(|_| T::default()),
            base: 0,
            len: 0,
        }
    }
}

impl<T, const N: usize> Stack<T, N> {
    pub(crate) fn reset(&mut self) {
        self.base = 0;
        self.len = 0;
    }

    pub(crate) fn clear(&mut self) {
        self.len = self.base;
    }

    #[cfg(feature = "experimental-regions")]
    pub(crate) fn truncate(&mut self, length: usize) {
        self.len = self.len.min(self.base + length);
    }

    pub(crate) fn is_full(&self) -> bool {
        self.len == N
    }

    pub(crate) fn push(&mut self, value: T) {
        self.data[self.len] = value;
        self.len += 1;
    }

    pub(crate) fn suspend(&mut self) -> Mark {
        let mark = Mark {
            base: self.base as u16,
            len: self.len as u16,
        };
        self.base = self.len;
        mark
    }

    pub(crate) fn restore(&mut self, mark: Mark) {
        self.base = usize::from(mark.base);
        self.len = usize::from(mark.len);
    }
}

impl<T: Default, const N: usize> Stack<T, N> {
    pub(crate) fn pop(&mut self) -> Option<T> {
        if self.len == self.base {
            None
        } else {
            self.len -= 1;
            Some(std::mem::take(&mut self.data[self.len]))
        }
    }
}

impl<T, const N: usize> Deref for Stack<T, N> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.data[self.base..self.len]
    }
}

impl<T, const N: usize> DerefMut for Stack<T, N> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.data[self.base..self.len]
    }
}

#[cfg(test)]
mod tests {
    use super::Stack;

    #[test]
    fn nested_segments_restore_parents_and_isolate_roots() {
        let mut stack = Stack::<u32, 4>::default();
        stack.push(1);
        let parent = stack.suspend();
        assert!(stack.is_empty());
        assert_eq!(stack.pop(), None);
        stack.push(2);
        let child = stack.suspend();
        stack.push(3);
        stack.push(4);
        assert_eq!(&*stack, &[3, 4]);
        stack.clear();
        assert_eq!(stack.pop(), None);
        stack.restore(child);
        assert_eq!(&*stack, &[2]);
        stack.restore(parent);
        assert_eq!(&*stack, &[1]);

        // A failed scan may abandon a child segment. Reset discards all bases.
        stack.suspend();
        stack.push(5);
        stack.reset();
        assert!(stack.is_empty());
        stack.push(6);
        assert_eq!(&*stack, &[6]);
    }
}
