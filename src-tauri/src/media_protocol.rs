//! media:// 自定义协议的纯函数部分：只读文件 + Range 处理，不依赖 Tauri 运行时，可单测。
//! 规则：无 Range → 200 全量；单段 Range → 206；多段 Range → 501；
//! 无法满足的 Range → 416；文件缺失 → 404；HEAD → 同 GET 头部、空 body。

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use tauri::http::{Response, StatusCode, header};

/// 常见媒体扩展名 → MIME；未知一律 application/octet-stream。
fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("m4v") => "video/x-m4v",
        Some("webm") => "video/webm",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("m4a") => "audio/mp4",
        Some("aac") => "audio/aac",
        _ => "application/octet-stream",
    }
}

fn builder_for(status: StatusCode, mime: &str) -> tauri::http::response::Builder {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        .header(header::ACCEPT_RANGES, "bytes")
}

/// 资产不在项目内（或路径非法）→ 404，不带任何文件系统信息。
pub fn not_found() -> Response<Vec<u8>> {
    builder_for(StatusCode::NOT_FOUND, "text/plain; charset=utf-8")
        .body(Vec::new())
        .expect("404 response")
}

/// 解析 Range 头为 [start, end] 闭区间。
/// Ok(None) = 无 range 或未知 unit（按 RFC 9110 忽略，回退 200 全量）。
/// Err(()) = 多段（调用方 501）或语法/边界非法（调用方 416），由 `multi` 区分。
fn parse_range(value: &str, size: u64) -> Result<Option<(u64, u64)>, RangeParseError> {
    let Some(spec) = value.strip_prefix("bytes=") else {
        return Ok(None); // 未知 unit：忽略
    };
    if spec.contains(',') {
        return Err(RangeParseError::Multi);
    }
    let Some((start_text, end_text)) = spec.split_once('-') else {
        return Err(RangeParseError::Invalid);
    };
    if start_text.is_empty() {
        // 后缀区间：最后 N 字节
        let suffix: u64 = end_text.parse().map_err(|_| RangeParseError::Invalid)?;
        if suffix == 0 {
            return Err(RangeParseError::Invalid);
        }
        let start = size.saturating_sub(suffix);
        return Ok(Some((start, size.saturating_sub(1))));
    }
    let start: u64 = start_text.parse().map_err(|_| RangeParseError::Invalid)?;
    let end: u64 = if end_text.is_empty() {
        size.saturating_sub(1)
    } else {
        end_text.parse().map_err(|_| RangeParseError::Invalid)?
    };
    if size == 0 || start >= size || start > end {
        return Err(RangeParseError::Unsatisfiable);
    }
    Ok(Some((start, end.min(size - 1))))
}

enum RangeParseError {
    Multi,
    Invalid,
    Unsatisfiable,
}

/// 读取文件区间；is_head 时只出头部的空 body。
pub fn media_response(method: &str, range_header: Option<&str>, path: &Path) -> Response<Vec<u8>> {
    let is_head = method.eq_ignore_ascii_case("HEAD");
    let mime = mime_for(path);
    let size = match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => metadata.len(),
        _ => return not_found(),
    };

    let range = match range_header {
        Some(value) => match parse_range(value, size) {
            Ok(range) => range,
            Err(RangeParseError::Multi) => {
                return builder_for(StatusCode::NOT_IMPLEMENTED, mime)
                    .body(Vec::new())
                    .expect("501 response");
            }
            Err(_) => {
                return builder_for(StatusCode::RANGE_NOT_SATISFIABLE, mime)
                    .header(header::CONTENT_RANGE, format!("bytes */{size}"))
                    .body(Vec::new())
                    .expect("416 response");
            }
        },
        None => None,
    };

    let (status, start, end) = match range {
        Some((start, end)) => (StatusCode::PARTIAL_CONTENT, start, end),
        None => (StatusCode::OK, 0, size.saturating_sub(1)),
    };
    let length = if size == 0 { 0 } else { end - start + 1 };

    let body = if is_head || length == 0 {
        Vec::new()
    } else {
        let read = std::fs::File::open(path).and_then(|mut file| {
            file.seek(SeekFrom::Start(start))?;
            let mut buffer = Vec::with_capacity(length.min(64 * 1024 * 1024) as usize);
            file.take(length).read_to_end(&mut buffer)?;
            Ok(buffer)
        });
        match read {
            Ok(bytes) => bytes,
            Err(_) => return not_found(),
        }
    };

    // HEAD 的 Content-Length 同样反映 GET 会返回的长度（body 为空但长度照报）。
    let mut builder = builder_for(status, mime).header(header::CONTENT_LENGTH, length);
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"));
    }
    builder.body(body).expect("media response")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_file() -> (std::path::PathBuf, Vec<u8>) {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "double-love-media-{}-{unique}.bin",
            std::process::id()
        ));
        let bytes: Vec<u8> = (0_u8..=99).collect();
        std::fs::write(&path, &bytes).expect("fixture written");
        (path, bytes)
    }

    fn body(response: Response<Vec<u8>>) -> Vec<u8> {
        response.into_body()
    }

    #[test]
    fn full_get_returns_200_with_accept_ranges() {
        let (path, bytes) = fixture_file();
        let response = media_response("GET", None, &path);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::ACCEPT_RANGES], "bytes");
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "100");
        assert_eq!(body(response), bytes);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn single_range_returns_206_and_exact_bytes() {
        let (path, bytes) = fixture_file();
        let response = media_response("GET", Some("bytes=10-19"), &path);
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 10-19/100");
        assert_eq!(body(response), bytes[10..=19].to_vec());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn open_and_suffix_ranges_are_supported() {
        let (path, bytes) = fixture_file();
        let open = media_response("GET", Some("bytes=90-"), &path);
        assert_eq!(open.headers()[header::CONTENT_RANGE], "bytes 90-99/100");
        assert_eq!(body(open), bytes[90..=99].to_vec());

        let suffix = media_response("GET", Some("bytes=-5"), &path);
        assert_eq!(suffix.headers()[header::CONTENT_RANGE], "bytes 95-99/100");
        assert_eq!(body(suffix), bytes[95..=99].to_vec());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn end_beyond_size_is_clamped() {
        let (path, bytes) = fixture_file();
        let response = media_response("GET", Some("bytes=95-999"), &path);
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 95-99/100");
        assert_eq!(body(response), bytes[95..=99].to_vec());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn multi_range_is_501() {
        let (path, _) = fixture_file();
        let response = media_response("GET", Some("bytes=0-9,20-29"), &path);
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn unsatisfiable_and_malformed_ranges_are_416() {
        let (path, _) = fixture_file();
        for value in ["bytes=100-200", "bytes=50-40", "bytes=abc-1", "bytes=-0"] {
            let response = media_response("GET", Some(value), &path);
            assert_eq!(
                response.status(),
                StatusCode::RANGE_NOT_SATISFIABLE,
                "range {value}"
            );
            assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes */100");
        }
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn unknown_unit_is_ignored_as_full_get() {
        let (path, bytes) = fixture_file();
        let response = media_response("GET", Some("items=0-9"), &path);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body(response), bytes);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn head_returns_headers_with_empty_body() {
        let (path, _) = fixture_file();
        let response = media_response("HEAD", Some("bytes=0-9"), &path);
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "10");
        assert!(body(response).is_empty());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn missing_file_is_404() {
        let response = media_response("GET", None, Path::new("/definitely/not/here.mp4"));
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
