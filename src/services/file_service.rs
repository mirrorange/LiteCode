use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug)]
pub struct FileService {
    working_dir: Arc<Mutex<PathBuf>>,
}

impl FileService {
    pub fn new(working_dir: Arc<Mutex<PathBuf>>) -> Self {
        Self { working_dir }
    }

    pub fn working_dir(&self) -> PathBuf {
        self.working_dir
            .lock()
            .expect("working directory lock poisoned")
            .clone()
    }
}
