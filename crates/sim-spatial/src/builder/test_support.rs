use super::*;

/// Test hooks for code outside the builder (the mode-switch test).
#[cfg(test)]
impl Builder {
    /// Open a text-field draft: one of the `system_open` blockers.
    pub(crate) fn test_open_draft(&mut self, text: &str) {
        self.start_input(Purpose::Filter, text.into());
    }
    pub(crate) fn test_drop_draft(&mut self) {
        self.input = None;
    }
    /// A live run is kept: whether its thread reports it running (None: no run).
    pub(crate) fn test_run(&self) -> Option<bool> {
        self.run.as_ref().map(|_| self.running())
    }
}
