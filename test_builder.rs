use weave_graph_core::{Storage, StorageBuilder, StorageError};
use std::cell::RefCell;

pub struct TestBuilder(pub RefCell<Option<Box<dyn Storage>>>);
impl StorageBuilder for TestBuilder {
    fn open(&self, _path: &std::path::Path) -> Result<Box<dyn Storage>, StorageError> { unimplemented!() }
    fn open_rebuild(&self, _path: &std::path::Path) -> Result<Box<dyn Storage>, StorageError> { unimplemented!() }
    fn open_in_memory(&self) -> Result<Box<dyn Storage>, StorageError> { Ok(self.0.borrow_mut().take().unwrap()) }
    fn open_read_only(&self, _path: &std::path::Path) -> Result<Box<dyn Storage>, StorageError> { unimplemented!() }
}
