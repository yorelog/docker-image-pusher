// This file implements the ChunkedUploader struct for handling chunked uploads of Docker image tar packages to a registry.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

const CHUNK_SIZE: usize = 5 * 1024 * 1024; // 5 MB chunks

pub struct ChunkedUploader {
    file: File,
    total_size: u64,
    uploaded_size: u64,
}

impl ChunkedUploader {
    pub fn new<P: AsRef<Path>>(file_path: P) -> io::Result<Self> {
        let file = File::open(file_path)?;
        let total_size = file.metadata()?.len();
        Ok(ChunkedUploader {
            file,
            total_size,
            uploaded_size: 0,
        })
    }

    pub fn upload_chunk(&mut self) -> io::Result<usize> {
        let mut buffer = vec![0; CHUNK_SIZE];
        self.file.seek(SeekFrom::Start(self.uploaded_size))?;
        let bytes_read = self.file.read(&mut buffer)?;

        if bytes_read > 0 {
            // Here you would implement the logic to upload the chunk to the registry.
            // For example, sending the buffer to the registry API.
            self.uploaded_size += bytes_read as u64;
        }

        Ok(bytes_read)
    }

    pub fn total_size(&self) -> u64 {
        self.total_size
    }

    pub fn uploaded_size(&self) -> u64 {
        self.uploaded_size
    }

    pub fn is_complete(&self) -> bool {
        self.uploaded_size >= self.total_size
    }
}