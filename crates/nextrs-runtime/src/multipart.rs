//! `multipart/form-data` parsing for `Request.formData()` and
//! `Response.formData()`. Next.js decodes progressively enhanced form
//! submissions (no-JavaScript Server Actions) this way: it builds a
//! `Request` from the size-limited action body and calls `formData()`.
//!
//! The body is already in memory (Body.formData() must read all of it, and
//! Next bounds it by `serverActions.bodySizeLimit` before parsing); this
//! bounds what the parse itself can produce. The parser is `multer`.

use bytes::Bytes;
use deno_core::{JsBuffer, ToJsBuffer, op2};
use deno_error::JsErrorBox;
use serde::Serialize;

/// Parts per body. React encodes each Server Action argument as at least one
/// part and Next allows 1000 arguments; a form with more parts than this is
/// refused rather than parsed.
pub(crate) const MAX_PARTS: usize = 4096;
/// Bytes of a part name (the field name); filenames share the limit.
pub(crate) const MAX_NAME_BYTES: usize = 8 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Part {
    name: String,
    /// Present for file parts (`filename=` in Content-Disposition), even
    /// when empty, which is how a file input with no file is sent.
    filename: Option<String>,
    content_type: Option<String>,
    data: ToJsBuffer,
}

fn type_error(message: impl std::fmt::Display) -> JsErrorBox {
    JsErrorBox::type_error(format!("Failed to parse body as FormData: {message}"))
}

/// Parses a whole multipart body. Malformed input (bad or missing boundary,
/// truncated parts, a missing final delimiter) is a TypeError, as in
/// Node's `formData()`.
#[op2]
#[serde]
pub(crate) async fn op_nextrs_multipart_parse(
    #[string] content_type: String,
    #[buffer] body: JsBuffer,
) -> Result<Vec<Part>, JsErrorBox> {
    parse(&content_type, Bytes::copy_from_slice(&body)).await
}

pub(crate) async fn parse(content_type: &str, body: Bytes) -> Result<Vec<Part>, JsErrorBox> {
    let boundary = multer::parse_boundary(content_type).map_err(type_error)?;
    let stream = futures_util::stream::once(async move { Ok::<_, std::io::Error>(body) });
    let mut multipart = multer::Multipart::new(stream, boundary);
    let mut parts = Vec::new();
    while let Some(field) = multipart.next_field().await.map_err(type_error)? {
        if parts.len() >= MAX_PARTS {
            return Err(type_error(format!("more than {MAX_PARTS} parts")));
        }
        let name = field
            .name()
            .ok_or_else(|| type_error("a part has no name"))?
            .to_owned();
        let filename = field.file_name().map(str::to_owned);
        if name.len() > MAX_NAME_BYTES
            || filename.as_ref().is_some_and(|f| f.len() > MAX_NAME_BYTES)
        {
            return Err(type_error(format!(
                "a part name or filename exceeds {MAX_NAME_BYTES} bytes"
            )));
        }
        let content_type = field.content_type().map(ToString::to_string);
        let data = field.bytes().await.map_err(type_error)?;
        parts.push(Part {
            name,
            filename,
            content_type,
            data: data.to_vec().into(),
        });
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(content_type: &str, body: &str) -> Result<Vec<Part>, JsErrorBox> {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(parse(content_type, Bytes::from(body.to_owned())))
    }

    const CT: &str = "multipart/form-data; boundary=XyZ";

    #[test]
    fn fields_files_and_repeats() {
        let body = "--XyZ\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n1\r\n\
                    --XyZ\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n2\r\n\
                    --XyZ\r\nContent-Disposition: form-data; name=\"t\"\r\n\r\n\u{1F4DA} caf\u{e9}\r\n\
                    --XyZ\r\nContent-Disposition: form-data; name=\"f\"; filename=\"x.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n\0\x01\r\n\
                    --XyZ\r\nContent-Disposition: form-data; name=\"e\"\r\n\r\n\r\n\
                    --XyZ--\r\n";
        let parts = run(CT, body).unwrap();
        let names: Vec<_> = parts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["a", "a", "t", "f", "e"]);
        assert_eq!(parts[3].filename.as_deref(), Some("x.bin"));
        assert_eq!(
            parts[3].content_type.as_deref(),
            Some("application/octet-stream")
        );
        assert!(parts[0].filename.is_none());
    }

    #[test]
    fn malformed_bodies_are_type_errors() {
        assert!(run("multipart/form-data", "--XyZ--\r\n").is_err());
        // Truncated: no closing delimiter.
        assert!(
            run(
                CT,
                "--XyZ\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n1"
            )
            .is_err()
        );
        // Wrong boundary.
        assert!(
            run(
                CT,
                "--Other\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n1\r\n--Other--\r\n"
            )
            .is_err()
        );
    }

    #[test]
    fn part_count_is_bounded() {
        let mut body = String::new();
        for _ in 0..=MAX_PARTS {
            body.push_str("--XyZ\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nx\r\n");
        }
        body.push_str("--XyZ--\r\n");
        assert!(run(CT, &body).is_err());
    }
}
