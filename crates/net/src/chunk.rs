use std::io::{self, Read};

pub const DEFAULT_CHUNK_BYTES: usize = 32 * 1024;

pub struct Chunker<R> {
    reader: R,
    chunk_size: usize,
    next_seq: u64,
}

impl<R: Read> Chunker<R> {
    pub fn new(reader: R, chunk_size: usize, resume_seq: u64) -> Self {
        Self {
            reader,
            chunk_size,
            next_seq: resume_seq,
        }
    }
}

impl<R: Read> Iterator for Chunker<R> {
    type Item = io::Result<(u64, Vec<u8>)>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut buf = vec![0u8; self.chunk_size];
        let mut filled = 0;

        while filled < self.chunk_size {
            match self.reader.read(&mut buf[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Some(Err(e)),
            }
        }

        if filled == 0 {
            return None;
        }

        buf.truncate(filled);
        let seq = self.next_seq;
        self.next_seq += 1;
        Some(Ok((seq, buf)))
    }
}

pub fn resume_seq_from_offset(offset_bytes: u64, chunk_size: usize) -> u64 {
    offset_bytes / chunk_size as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn splits_data_into_fixed_size_chunks() {
        let data = vec![0u8; 25];
        let chunker = Chunker::new(Cursor::new(data), 10, 0);
        let chunks: Vec<_> = chunker.map(|c| c.unwrap()).collect();

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0], (0, vec![0u8; 10]));
        assert_eq!(chunks[1], (1, vec![0u8; 10]));
        assert_eq!(chunks[2], (2, vec![0u8; 5]));
    }

    #[test]
    fn empty_input_yields_no_chunks() {
        let chunker = Chunker::new(Cursor::new(Vec::<u8>::new()), 10, 0);
        assert_eq!(chunker.count(), 0);
    }

    #[test]
    fn resume_seq_starts_sequence_numbering_from_the_given_offset() {
        let data = vec![7u8; 15];
        let chunker = Chunker::new(Cursor::new(data), 10, 3);
        let chunks: Vec<_> = chunker.map(|c| c.unwrap()).collect();

        assert_eq!(chunks[0].0, 3);
        assert_eq!(chunks[1].0, 4);
    }

    #[test]
    fn resume_seq_from_offset_matches_chunk_boundaries() {
        assert_eq!(resume_seq_from_offset(0, 10), 0);
        assert_eq!(resume_seq_from_offset(9, 10), 0);
        assert_eq!(resume_seq_from_offset(10, 10), 1);
        assert_eq!(resume_seq_from_offset(25, 10), 2);
    }

    #[test]
    fn preserves_exact_byte_content_across_chunk_boundaries() {
        let data: Vec<u8> = (0..250u32).map(|n| (n % 256) as u8).collect();
        let chunker = Chunker::new(Cursor::new(data.clone()), 64, 0);
        let mut reassembled = Vec::new();
        for chunk in chunker {
            reassembled.extend(chunk.unwrap().1);
        }
        assert_eq!(reassembled, data);
    }
}
