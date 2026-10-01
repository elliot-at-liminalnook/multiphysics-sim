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

/// The shared selection and a document registry with the builder's file
/// open as the Build document (what the window's Build arrival makes), for
/// tests that drive a builder without an `App`.
#[cfg(test)]
pub(crate) fn test_selection(b: &Builder) -> (Selection, DocumentRegistry) {
    let mut registry = DocumentRegistry::default();
    registry.open(ViewerMode::Build, crate::document::DocumentKind::System, crate::document::Source::path(b.path()));
    (Selection::default(), registry)
}
