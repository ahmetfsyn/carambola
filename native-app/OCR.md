# Offline OCR support

Carambola recognizes the pixels in the selected crop locally. It does not send
the image or recognized text to a network service. OCR runs on a worker thread
so the overlay remains responsive. A successful, nonempty result is copied to
the clipboard and followed by a native notification; an empty result leaves the
clipboard untouched.

| Platform | Engine | Runtime/model requirement |
| --- | --- | --- |
| Windows | Windows.Media.Ocr | Install the desired OCR language pack in Windows. No model is bundled. |
| macOS | Vision framework | Uses the system framework and its installed recognition support. No model is bundled. |
| Linux | Tesseract command line, English (`eng`) | Install `tesseract` and its `eng.traineddata` file. The executable and data remain system-installed. |

Windows and macOS use operating-system OCR APIs. Linux writes the crop to an
in-memory PNG and pipes it to Tesseract using its LSTM engine and single-block
page segmentation mode. The operating-system OCR models and Tesseract data are
not added to Carambola's package, so the OCR integration adds no model payload
to the application binary. The macOS bridge is a small Objective-C adapter to
Vision; Linux invokes an external process and therefore depends on that local
installation.

There are no benchmark measurements yet. Recognition time depends on crop size,
installed language data, and the platform runtime. The first OCR request may
include system engine initialization cost.
