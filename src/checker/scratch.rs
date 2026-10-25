//! Own shared buffers and save/restore their active annotation segments.

use super::annotations::{AnnotationState, AnnotationTracker};
use super::widths::{WidthState, WidthTracker};
use crate::Limits;
use crate::lexer::LexerScratch;
use crate::rules::{Scanner, ScannerState};
use crate::stack::{Mark, Stack};

// One set of shared buffers serves every scan. Work frames save only scalar
// state and segment marks; suspended entries remain below the active base.
pub(super) struct ScanScratch {
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
        self.lexer.clear_segments();
        self.scanner.clear_segments();
        self.annotations.brackets.reset();
        self.widths.frames.reset();
    }

    pub(super) fn new(limits: &Limits, preallocated: bool) -> Self {
        let frames = annotation_frames(limits);
        // Children share the remaining depth. Each suspended scan additionally
        // needs a root entry; one transient entry lets the lexer report overflow.
        let capacity = usize::from(limits.max_nesting_depth) + frames + 1;

        Self {
            lexer: LexerScratch::with_storage(
                capacity,
                usize::from(limits.max_nesting_depth.min(limits.max_indentation_depth))
                    + frames
                    + 1,
                preallocated,
            ),
            scanner: Scanner::with_storage(
                u32::from(limits.max_nesting_depth),
                limits.max_ast_depth.map(u32::from),
                capacity,
                preallocated,
            ),
            annotations: AnnotationTracker {
                brackets: Stack::with_storage(capacity, preallocated),
                state: AnnotationState::default(),
            },
            // Pending lambda parameter lists do not charge parser depth until
            // their colons arrive. Preserve the full bounded width capacity.
            widths: WidthTracker::with_storage(preallocated),
        }
    }
}

pub(super) fn annotation_frames(limits: &Limits) -> usize {
    if limits.check_string_annotations {
        usize::from(limits.max_annotation_depth) + 1
    } else {
        1
    }
}
