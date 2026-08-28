# Performance notes

## Baseline

On the validation desktop, a full 958 x 925 BGRA frame is 3,544,600 bytes.
The Windows backend crops on the GPU before that buffer is read back.

## Decisions

- `Frame::into_readback()` transfers the existing CPU buffer; callers that own a
  frame avoid the extra `Vec<u8>` clone from `readback()`.
- `try_next_frame()` returns no buffer on an unchanged desktop, so consumers do
  not allocate a duplicate frame simply to preserve elapsed time.

## Ceiling

The current QuickGIFlick proof-of-pipeline keeps changed frames in memory until
GIF encoding. A 958 x 925 capture at 15 FPS for three seconds has a worst-case
raw frame payload of about 160 MB. The recording length is intentionally fixed
to three seconds; replace this with a bounded encoder queue before adding
user-configurable or longer recordings.
