//! Bounded Vec-backed segmented stacks; annotation scans share buffers without copying entries.
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
    data: Vec<T>,
    base: usize,
}

impl<T, const N: usize> Default for Stack<T, N> {
    fn default() -> Self {
        Self::with_capacity(N)
    }
}

impl<T, const N: usize> Stack<T, N> {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        const {
            assert!(N <= u16::MAX as usize);
        }
        assert!(capacity <= N);

        Self {
            data: Vec::with_capacity(capacity),
            base: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.data.clear();
        self.base = 0;
    }

    pub(crate) fn clear(&mut self) {
        self.data.truncate(self.base);
    }

    #[cfg(feature = "regions")]
    pub(crate) fn truncate(&mut self, length: usize) {
        self.data.truncate(self.base + length);
    }

    pub(crate) fn is_full(&self) -> bool {
        self.data.len() == N
    }

    pub(crate) fn push(&mut self, value: T) {
        // Capacities are fixed at construction, independently of input. Never
        // silently allocate if a structural accounting invariant is broken.
        assert!(self.data.len() < self.data.capacity() && !self.is_full());
        self.data.push(value);
    }

    pub(crate) fn suspend(&mut self) -> Mark {
        let mark = Mark {
            base: self.base as u16,
            len: self.data.len() as u16,
        };
        self.base = self.data.len();
        mark
    }

    pub(crate) fn restore(&mut self, mark: Mark) {
        self.base = usize::from(mark.base);
        self.data.truncate(usize::from(mark.len));
    }
}

impl<T, const N: usize> Stack<T, N> {
    pub(crate) fn pop(&mut self) -> Option<T> {
        if self.data.len() == self.base {
            None
        } else {
            self.data.pop()
        }
    }
}

impl<T, const N: usize> Deref for Stack<T, N> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.data[self.base..]
    }
}

impl<T, const N: usize> DerefMut for Stack<T, N> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.data[self.base..]
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
