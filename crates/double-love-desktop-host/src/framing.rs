use std::io::{self, Read, Write};

use thiserror::Error;

pub const MAX_FRAME_BYTES: u32 = 64 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum FrameReadError {
    #[error("frame declares {declared} bytes; maximum is {maximum}")]
    TooLarge { declared: u32, maximum: u32 },
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub fn read_frame(reader: &mut impl Read) -> Result<Option<Vec<u8>>, FrameReadError> {
    let mut prefix = [0_u8; 4];
    loop {
        match reader.read(&mut prefix[..1]) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(FrameReadError::Io(error)),
        }
    }
    reader.read_exact(&mut prefix[1..])?;

    let declared = u32::from_be_bytes(prefix);
    if declared > MAX_FRAME_BYTES {
        return Err(FrameReadError::TooLarge {
            declared,
            maximum: MAX_FRAME_BYTES,
        });
    }

    let mut body = vec![0_u8; declared as usize];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

pub fn write_frame(writer: &mut impl Write, body: &[u8]) -> io::Result<()> {
    let length = u32::try_from(body.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "frame body cannot be represented by a 4-byte prefix",
        )
    })?;
    if length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frame body exceeds the 64 MiB limit",
        ));
    }

    writer.write_all(&length.to_be_bytes())?;
    writer.write_all(body)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read};

    use super::{FrameReadError, MAX_FRAME_BYTES, read_frame};

    struct PrefixOnlyReader {
        prefix: [u8; 4],
        offset: usize,
    }

    impl Read for PrefixOnlyReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            assert!(self.offset < self.prefix.len(), "frame body was read");
            let count = buffer.len().min(self.prefix.len() - self.offset);
            buffer[..count].copy_from_slice(&self.prefix[self.offset..self.offset + count]);
            self.offset += count;
            Ok(count)
        }
    }

    #[test]
    fn oversized_frame_is_rejected_before_body_read_or_allocation() {
        let mut reader = PrefixOnlyReader {
            prefix: (MAX_FRAME_BYTES + 1).to_be_bytes(),
            offset: 0,
        };

        let error = read_frame(&mut reader).expect_err("oversized frame should fail");
        assert!(matches!(
            error,
            FrameReadError::TooLarge {
                declared,
                maximum: MAX_FRAME_BYTES
            } if declared == MAX_FRAME_BYTES + 1
        ));
        assert_eq!(reader.offset, 4);
    }
}
