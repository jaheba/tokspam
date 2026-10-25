//! Own shared buffers and save/restore their active annotation segments.

use super::annotations::{AnnotationState, AnnotationTracker};
use super::escapes::DecodedText;
use super::widths::{WidthState, WidthTracker};
use crate::Storage;
use crate::lexer::LexerScratch;
use crate::rules::{Scanner, ScannerState};
use crate::stack::{Mark, Stack};

// One set of shared buffers serves every scan. Work frames save only scalar
// state and segment marks; suspended entries remain below the active base.
pub(super) struct ScanScratch {
    // Decoded strings share one append-only arena. Frames remember its length
    // on entry so returning from a child releases only that child's bytes.
    pub(super) decoded: DecodedText,
    pub(super) lexer: LexerScratch,
    pub(super) scanner: Scanner,
    pub(super) annotations: AnnotationTracker,
    pub(super) widths: WidthTracker,
}

pub(super) struct SavedScratch {
    lexer: [Mark; 2],
    scanner: (ScannerState, [Mark; 3]),
    annotations: (AnnotationState, Mark),
    widths: (WidthState, Mark),
}

impl ScanScratch {
    pub(super) fn suspend(&mut self) -> SavedScratch {
        SavedScratch {
            lexer: self.lexer.suspend(),
            scanner: self.scanner.suspend(),
            annotations: (self.annotations.state, self.annotations.brackets.suspend()),
            widths: (self.widths.state, self.widths.frames.suspend()),
        }
    }

    pub(super) fn restore(&mut self, saved: SavedScratch) {
        self.lexer.restore(saved.lexer);
        self.scanner.restore(saved.scanner);
        self.annotations.state = saved.annotations.0;
        self.annotations.brackets.restore(saved.annotations.1);
        self.widths.state = saved.widths.0;
        self.widths.frames.restore(saved.widths.1);
    }

    pub(super) fn clear_segments(&mut self) {
        self.decoded.reset();
        self.lexer.clear_segments();
        self.scanner.clear_segments();
        self.annotations.brackets.reset();
        self.widths.frames.reset();
    }

    pub(super) fn failed(&self) -> bool {
        self.decoded.failed()
            || self.lexer.failed()
            || self.scanner.failed()
            || self.annotations.brackets.failed()
            || self.widths.frames.failed()
    }

    pub(super) fn new(storage: Storage) -> Self {
        let (capacity, fixed) = match storage {
            Storage::Growing => (0, false),
            Storage::Fixed { capacity } => (capacity, true),
        };
        Self {
            decoded: DecodedText::with_storage(capacity, fixed),
            lexer: LexerScratch::with_storage(capacity, capacity, fixed),
            scanner: Scanner::with_storage(capacity, fixed),
            annotations: AnnotationTracker {
                brackets: Stack::with_storage(capacity, fixed),
                state: AnnotationState::default(),
            },
            widths: WidthTracker::with_storage(capacity, fixed),
        }
    }
}
