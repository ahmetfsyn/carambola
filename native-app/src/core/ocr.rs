#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrResult {
    Text(String),
    Empty,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrCompletion {
    Copied { text: String },
    Empty,
    Error(String),
}

impl OcrCompletion {
    pub fn notification_message(&self) -> String {
        match self {
            Self::Copied { .. } => "Text scanned and copied successfully.".into(),
            Self::Empty => "No text found in the image.".into(),
            Self::Error(error) => format!("Text scan failed: {error}"),
        }
    }
}

pub fn normalize_result(result: Result<String, String>) -> OcrResult {
    match result {
        Ok(text) => {
            let text = text.trim().to_string();
            if text.is_empty() {
                OcrResult::Empty
            } else {
                OcrResult::Text(text)
            }
        }
        Err(error) => OcrResult::Failed(error),
    }
}

pub fn complete(
    result: OcrResult,
    copy_to_clipboard: impl FnOnce(&str) -> Result<(), String>,
) -> OcrCompletion {
    match result {
        OcrResult::Text(text) => match copy_to_clipboard(&text) {
            Ok(()) => OcrCompletion::Copied { text },
            Err(error) => OcrCompletion::Error(format!(
                "Text was recognized but could not be copied: {error}"
            )),
        },
        OcrResult::Empty => OcrCompletion::Empty,
        OcrResult::Failed(error) => OcrCompletion::Error(error),
    }
}

#[cfg(test)]
mod tests {
    use super::{complete, normalize_result, OcrCompletion, OcrResult};
    use std::cell::Cell;

    #[test]
    fn recognition_keeps_reading_order_line_breaks_and_trims_only_outer_space() {
        assert_eq!(
            normalize_result(Ok("  first line\nsecond line  \n".into())),
            OcrResult::Text("first line\nsecond line".into())
        );
    }

    #[test]
    fn empty_recognition_never_requests_a_clipboard_write() {
        let clipboard_called = Cell::new(false);
        assert_eq!(normalize_result(Ok(" \n  ".into())), OcrResult::Empty);
        assert_eq!(
            complete(OcrResult::Empty, |_| {
                clipboard_called.set(true);
                Err("must not be called".into())
            }),
            OcrCompletion::Empty
        );
        assert!(!clipboard_called.get());
    }

    #[test]
    fn successful_recognition_copies_then_notifies() {
        let completion = complete(OcrResult::Text("recognized".into()), |text| {
            assert_eq!(text, "recognized");
            Ok(())
        });
        assert_eq!(
            completion,
            OcrCompletion::Copied {
                text: "recognized".into()
            }
        );
        assert_eq!(
            completion.notification_message(),
            "Text scanned and copied successfully."
        );
    }

    #[test]
    fn empty_and_failed_recognition_have_distinct_notification_messages() {
        assert_eq!(
            OcrCompletion::Empty.notification_message(),
            "No text found in the image."
        );
        assert_eq!(
            OcrCompletion::Error("OCR runtime unavailable".into()).notification_message(),
            "Text scan failed: OCR runtime unavailable"
        );
    }

    #[test]
    fn recognition_and_clipboard_errors_are_reported_without_success_notifications() {
        let clipboard_called = Cell::new(false);
        assert_eq!(
            complete(OcrResult::Failed("OCR runtime unavailable".into()), |_| {
                clipboard_called.set(true);
                Ok(())
            }),
            OcrCompletion::Error("OCR runtime unavailable".into())
        );
        assert!(!clipboard_called.get());
        let copy_error = complete(OcrResult::Text("recognized".into()), |_| {
            Err("clipboard busy".into())
        });
        assert_eq!(
            copy_error,
            OcrCompletion::Error(
                "Text was recognized but could not be copied: clipboard busy".into()
            )
        );
        assert_eq!(
            copy_error.notification_message(),
            "Text scan failed: Text was recognized but could not be copied: clipboard busy"
        );
    }
}
