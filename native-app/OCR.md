# Windows OCR

Carambola recognizes the selected crop locally with Windows.Media.Ocr on a
background worker. No OCR model is bundled; install the desired language pack
through Windows settings if the system cannot find a suitable language.

A nonempty recognized result is copied to the clipboard. An empty result or
recognition error does not replace the clipboard contents. All OCR outcomes
close the overlay; after it closes, a native notification reports the outcome
when notifications are enabled. There is no centered busy popup.

The source includes experimental macOS Vision and Linux Tesseract adapters,
but those platforms do not have a complete verified capture and packaging
path and are not supported release targets.

No OCR performance measurements are published. Recognition time depends on
the crop, installed Windows language data, and operating-system runtime; the
first request may include engine startup.
