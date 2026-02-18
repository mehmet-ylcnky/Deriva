use bytes::Bytes;
use crate::DerivaError;

/// A chunk in a streaming computation pipeline.
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// A data chunk. May be any size, but typically `preferred_chunk_size`.
    Data(Bytes),
    /// End of stream. No more chunks will follow.
    End,
    /// Stream error. No more chunks will follow.
    Error(DerivaError),
}

impl StreamChunk {
    pub fn is_data(&self) -> bool {
        matches!(self, StreamChunk::Data(_))
    }

    pub fn is_end(&self) -> bool {
        matches!(self, StreamChunk::End)
    }

    pub fn is_error(&self) -> bool {
        matches!(self, StreamChunk::Error(_))
    }

    pub fn into_data(self) -> Option<Bytes> {
        match self {
            StreamChunk::Data(b) => Some(b),
            _ => None,
        }
    }

    pub fn data_len(&self) -> usize {
        match self {
            StreamChunk::Data(b) => b.len(),
            _ => 0,
        }
    }
}
