pub struct TestWriter {
    pub write_fn: Box<dyn FnMut(&[u8]) -> std::io::Result<usize> + Send>,
}

impl std::io::Write for TestWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        (self.write_fn)(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
