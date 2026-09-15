use std::ffi::{CStr, CString};
use std::io::{self, Write};
use std::num::NonZeroU8;

#[cfg(target_os = "android")]
use android_log_sys::LogPriority;
#[cfg(target_os = "android")]
use tracing::{Level, Metadata};
#[cfg(target_os = "android")]
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::{
    field::RecordFields,
    fmt::{
        format::{DefaultFields, Writer},
        FormatFields,
    },
};

const MESSAGE_MAX_LEN: usize = 4000;
// Android's 4068-byte payload includes one priority byte and two NULs.
const TAG_MAX_LEN: usize = 65;

pub(super) fn truncate_tag(tag: &CStr) -> CString {
    if tag.to_bytes().len() <= TAG_MAX_LEN {
        return tag.to_owned();
    }

    // Best-effort: a stderr failure must not prevent telemetry initialization.
    let _ = writeln!(
        io::stderr(),
        "logcat tag exceeds {TAG_MAX_LEN} bytes; truncating it"
    );

    let end = tag
        .to_str()
        .map_or(TAG_MAX_LEN, |text| text.floor_char_boundary(TAG_MAX_LEN));
    let bytes: Vec<NonZeroU8> = tag.to_bytes()[..end]
        .iter()
        .copied()
        .filter_map(NonZeroU8::new)
        .collect();

    CString::from(bytes)
}

fn write_chunks(
    bytes: &[u8],
    mut emit: impl FnMut(&CStr) -> io::Result<()>,
) -> io::Result<()> {
    let message = String::from_utf8_lossy(bytes).replace('\0', "\\0");

    let mut rest = message.as_str();

    while !rest.is_empty() {
        let mut end = rest.len().min(MESSAGE_MAX_LEN);

        while !rest.is_char_boundary(end) {
            end -= 1;
        }

        let (chunk, tail) = rest.split_at(end);

        let chunk = CString::new(chunk).expect("NUL bytes were escaped above");

        emit(&chunk)?;
        rest = tail;
    }

    Ok(())
}

#[cfg(target_os = "android")]
fn write_logcat_chunk(
    tag: &CStr,
    priority: android_log_sys::LogPriority,
    message: &CStr,
) -> io::Result<()> {
    // SAFETY: Both strings are NULL-terminated and remain valid for
    // the duration of the call. liblog does not retain their pointers

    let result = unsafe {
        android_log_sys::__android_log_write(
            priority as android_log_sys::c_int,
            tag.as_ptr(),
            message.as_ptr(),
        )
    };

    // -EPERM means Android filtered the message by tag or priority.
    if result < -1 {
        return Err(io::Error::from_raw_os_error(-result));
    }

    Ok(())
}

pub(super) struct LogcatFields;

impl<'writer> FormatFields<'writer> for LogcatFields {
    fn format_fields<R: RecordFields>(
        &self,
        writer: Writer<'writer>,
        fields: R,
    ) -> std::fmt::Result {
        DefaultFields::new().format_fields(writer, fields)
    }
}

#[cfg(target_os = "android")]
pub(super) struct LogcatWriter(pub(super) CString);

#[cfg(target_os = "android")]
impl<'a> MakeWriter<'a> for LogcatWriter {
    type Writer = EventWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        EventWriter {
            tag: &self.0,
            priority: LogPriority::INFO,
            buffer: Vec::new(),
        }
    }

    fn make_writer_for(&'a self, metadata: &Metadata<'_>) -> Self::Writer {
        let mut writer = self.make_writer();

        writer.priority = match *metadata.level() {
            Level::TRACE => LogPriority::VERBOSE,
            Level::DEBUG => LogPriority::DEBUG,
            Level::INFO => LogPriority::INFO,
            Level::WARN => LogPriority::WARN,
            Level::ERROR => LogPriority::ERROR,
        };
        writer
    }
}

#[cfg(target_os = "android")]
pub(super) struct EventWriter<'a> {
    tag: &'a CStr,
    priority: android_log_sys::LogPriority,
    buffer: Vec<u8>,
}

#[cfg(target_os = "android")]
impl Write for EventWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let buffer = std::mem::take(&mut self.buffer);

        write_chunks(&buffer, |message| {
            write_logcat_chunk(self.tag, self.priority, message)
        })
    }
}

#[cfg(target_os = "android")]
impl Drop for EventWriter<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.flush() {
            let _ = writeln!(std::io::stderr(), "failed writing to logcat: {error}");
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn it_accepts_a_tag_at_the_payload_limit() {
        let tag = CString::new("t".repeat(TAG_MAX_LEN)).unwrap();

        assert_eq!(truncate_tag(&tag), tag);

        assert_eq!(
            1 + tag.as_bytes_with_nul().len() + MESSAGE_MAX_LEN + 1,
            4068
        );
    }

    #[test]
    fn it_truncates_a_tag_over_the_limit() {
        let tag = CString::new("t".repeat(TAG_MAX_LEN + 1)).unwrap();

        let truncated = truncate_tag(&tag);

        assert_eq!(truncated.to_bytes(), "t".repeat(TAG_MAX_LEN).as_bytes());
    }

    #[test]
    fn it_truncates_tags_at_a_utf8_boundary() {
        let tag = CString::new("🦉".repeat(17)).unwrap();

        let truncated = truncate_tag(&tag);

        assert_eq!(truncated.to_str().unwrap(), "🦉".repeat(16));
        assert_eq!(truncated.to_bytes().len(), 64);
    }

    #[test]
    fn it_preserves_short_tags() {
        for tag in [c"", c"orb-backend-status", c"🦉"] {
            assert_eq!(truncate_tag(tag).as_c_str(), tag);
        }
    }

    #[test]
    fn it_truncates_non_utf8_tags_as_bytes() {
        let tag = CString::new(vec![0xff; TAG_MAX_LEN + 1]).unwrap();

        let truncated = truncate_tag(&tag);

        assert_eq!(truncated.to_bytes(), &[0xff; TAG_MAX_LEN]);
    }

    #[test]
    fn it_escapes_nulls_and_preserves_utf8_across_chunks() {
        // Arrange

        let prefix = "a".repeat(MESSAGE_MAX_LEN - 1);
        let message = format!("{prefix}🦉\0tail");
        let mut chunks = Vec::new();

        // Act

        write_chunks(message.as_bytes(), |chunk| {
            chunks.push(chunk.to_str().unwrap().to_owned());
            Ok(())
        })
        .unwrap();

        // Assert

        assert_eq!(chunks, vec![prefix, "🦉\\0tail".to_owned()]);
    }

    #[test]
    fn it_stops_after_chunk_write_fails() {
        // Arrange
        let message = vec![b'a'; MESSAGE_MAX_LEN * 3];
        let mut calls = 0;

        // Act
        let error = write_chunks(&message, |_| {
            calls += 1;
            if calls == 2 {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "test sink disconnected",
                ));
            }
            Ok(())
        })
        .unwrap_err();

        // Assert
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(calls, 2);
    }
}
