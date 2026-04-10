/// State for the dependency direction picker overlay (b).
#[derive(Debug)]
pub struct DepDirectionState {
    /// The bead ID we pressed 'b' on.
    pub bead_id: String,
}

/// State for the close confirmation overlay (Shift+X).
#[derive(Debug)]
pub struct CloseConfirmState {
    /// The bead ID to close.
    pub bead_id: String,
    /// Optional reason text being typed.
    pub reason: String,
    /// Cursor position (byte offset) within `reason`.
    pub cursor_pos: usize,
}

impl CloseConfirmState {
    pub(super) fn insert_char(&mut self, c: char) {
        self.reason.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    pub(super) fn delete_char_before(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.reason[..self.cursor_pos]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos -= prev;
            self.reason.remove(self.cursor_pos);
        }
    }

    pub(super) fn cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.reason[..self.cursor_pos]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos -= prev;
        }
    }

    pub(super) fn cursor_right(&mut self) {
        if self.cursor_pos < self.reason.len() {
            let next = self.reason[self.cursor_pos..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos += next;
        }
    }
}

/// State for the defer input overlay (d).
#[derive(Debug)]
pub struct DeferState {
    /// The bead ID to defer.
    pub bead_id: String,
    /// Optional "until" date text being typed.
    pub until: String,
    /// Cursor position (byte offset) within `until`.
    pub cursor_pos: usize,
}

impl DeferState {
    pub(super) fn insert_char(&mut self, c: char) {
        self.until.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    pub(super) fn delete_char_before(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.until[..self.cursor_pos]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos -= prev;
            self.until.remove(self.cursor_pos);
        }
    }

    pub(super) fn cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.until[..self.cursor_pos]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos -= prev;
        }
    }

    pub(super) fn cursor_right(&mut self) {
        if self.until.len() > self.cursor_pos {
            let next = self.until[self.cursor_pos..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos += next;
        }
    }
}
