//! Line framing: one JSON message per `\n`-terminated line, with a size limit.

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

/// Result of reading one frame.
#[derive(Debug, PartialEq, Eq)]
pub enum ReadFrame {
    /// A complete message (without the trailing newline).
    Message(Vec<u8>),
    /// The peer closed the connection.
    Closed,
    /// The message exceeded the limit; the connection must be closed after replying.
    TooLarge,
}

/// Reads one line of at most `max` bytes.
pub async fn read_frame<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max: usize,
) -> std::io::Result<ReadFrame> {
    let mut buf = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(if buf.is_empty() {
                ReadFrame::Closed
            } else {
                ReadFrame::Message(buf)
            });
        }
        if let Some(pos) = available.iter().position(|&b| b == b'\n') {
            if buf.len() + pos > max {
                return Ok(ReadFrame::TooLarge);
            }
            buf.extend_from_slice(&available[..pos]);
            reader.consume(pos + 1);
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            return Ok(ReadFrame::Message(buf));
        }
        let n = available.len();
        if buf.len() + n > max {
            return Ok(ReadFrame::TooLarge);
        }
        buf.extend_from_slice(available);
        reader.consume(n);
    }
}

/// Writes one message followed by `\n` and flushes.
pub async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, msg: &[u8]) -> std::io::Result<()> {
    writer.write_all(msg).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn reads_lines_and_close() {
        let mut r = BufReader::new(&b"{\"a\":1}\n{\"b\":2}\r\n"[..]);
        assert_eq!(
            read_frame(&mut r, 100).await.unwrap(),
            ReadFrame::Message(b"{\"a\":1}".to_vec())
        );
        assert_eq!(
            read_frame(&mut r, 100).await.unwrap(),
            ReadFrame::Message(b"{\"b\":2}".to_vec())
        );
        assert_eq!(read_frame(&mut r, 100).await.unwrap(), ReadFrame::Closed);
    }

    #[tokio::test]
    async fn oversized_line_is_rejected() {
        let big = [b'x'; 50];
        let mut r = BufReader::with_capacity(8, &big[..]);
        assert_eq!(read_frame(&mut r, 10).await.unwrap(), ReadFrame::TooLarge);
    }

    #[tokio::test]
    async fn write_appends_newline() {
        let mut out = Vec::new();
        write_frame(&mut out, b"{}").await.unwrap();
        assert_eq!(out, b"{}\n");
    }
}
