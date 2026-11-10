//! encode and decode the frames for the mux protocol.
//! The frames include the length of a PDU as well as an identifier
//! that informs us how to decode it.  The length, ident and serial
//! number are encoded using a variable length integer encoding.
//! Rather than rely solely on serde to serialize and deserialize an
//! enum, we encode the enum variants with a version/identifier tag
//! for ourselves.  This will make it a little easier to manage
//! client and server instances that are built from different versions
//! of this code; in this way the client and server can more gracefully
//! manage unknown enum variants.

#![allow(dead_code)]
#![allow(clippy::range_plus_one)]

use anyhow::{bail, Context as _, Error};
use config::keyassignment::{PaneDirection, ScrollbackEraseMode};
use mux::client::{ClientId, ClientInfo};
use mux::pane::PaneId;
use mux::renderable::{RenderableDimensions, StableCursorPosition};
use mux::tab::{PaneNode, SerdeUrl, SplitRequest, TabId};
use mux::window::WindowId;
use portable_pty::CommandBuilder;
use rangeset::*;
use serde::{Deserialize, Serialize};
use smol::io::AsyncWriteExt;
use smol::prelude::*;
use std::collections::HashMap;
use std::convert::TryInto;
use std::io::Cursor;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use termwiz::hyperlink::Hyperlink;
use termwiz::image::{ImageData, TextureCoordinate};
use termwiz::surface::{Line, SequenceNo};
use thiserror::Error;
use wezterm_term::color::ColorPalette;
use wezterm_term::{Alert, ClipboardSelection, StableRowIndex, TerminalSize};

#[derive(Error, Debug)]
#[error("Corrupt Response: {0}")]
pub struct CorruptResponse(String);

thread_local! {
    /// LBTerm: the payload bytes of the last PDU this thread encoded or decoded, which the mux
    /// client hands to the GUI's counters with the PDU's type (lbterm_extras::counters)
    static LBTERM_LAST_SIZE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// LBTerm: the payload bytes of the last PDU this thread encoded or decoded
pub fn lbterm_last_size() -> usize {
    LBTERM_LAST_SIZE.with(|size| size.get())
}

fn lbterm_note_size(bytes: usize) {
    LBTERM_LAST_SIZE.with(|size| size.set(bytes));
}

/// Returns the encoded length of the leb128 representation of value
fn encoded_length(value: u64) -> usize {
    struct NullWrite {}
    impl std::io::Write for NullWrite {
        fn write(&mut self, buf: &[u8]) -> std::result::Result<usize, std::io::Error> {
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::result::Result<(), std::io::Error> {
            Ok(())
        }
    }

    leb128::write::unsigned(&mut NullWrite {}, value).unwrap()
}

const COMPRESSED_MASK: u64 = 1 << 63;

fn encode_raw_as_vec(
    ident: u64,
    serial: u64,
    data: &[u8],
    is_compressed: bool,
) -> anyhow::Result<Vec<u8>> {
    let len = data.len() + encoded_length(ident) + encoded_length(serial);
    let masked_len = if is_compressed {
        (len as u64) | COMPRESSED_MASK
    } else {
        len as u64
    };

    // Double-buffer the data; since we run with nodelay enabled, it is
    // desirable for the write to be a single packet (or at least, for
    // the header portion to go out in a single packet)
    let mut buffer = Vec::with_capacity(len + encoded_length(masked_len));

    leb128::write::unsigned(&mut buffer, masked_len).context("writing pdu len")?;
    leb128::write::unsigned(&mut buffer, serial).context("writing pdu serial")?;
    leb128::write::unsigned(&mut buffer, ident).context("writing pdu ident")?;
    buffer.extend_from_slice(data);

    if is_compressed {
        metrics::histogram!("pdu.encode.compressed.size").record(buffer.len() as f64);
    } else {
        metrics::histogram!("pdu.encode.size").record(buffer.len() as f64);
    }

    Ok(buffer)
}

/// Encode a frame.  If the data is compressed, the high bit of the length
/// is set to indicate that.  The data written out has the format:
/// tagged_len: leb128  (u64 msb is set if data is compressed)
/// serial: leb128
/// ident: leb128
/// data bytes
fn encode_raw<W: std::io::Write>(
    ident: u64,
    serial: u64,
    data: &[u8],
    is_compressed: bool,
    mut w: W,
) -> anyhow::Result<usize> {
    let buffer = encode_raw_as_vec(ident, serial, data, is_compressed)?;
    w.write_all(&buffer).context("writing pdu data buffer")?;
    Ok(buffer.len())
}

async fn encode_raw_async<W: Unpin + AsyncWriteExt>(
    ident: u64,
    serial: u64,
    data: &[u8],
    is_compressed: bool,
    w: &mut W,
) -> anyhow::Result<usize> {
    let buffer = encode_raw_as_vec(ident, serial, data, is_compressed)?;
    w.write_all(&buffer)
        .await
        .context("writing pdu data buffer")?;
    Ok(buffer.len())
}

/// Read a single leb128 encoded value from the stream
async fn read_u64_async<R>(r: &mut R) -> anyhow::Result<u64>
where
    R: Unpin + AsyncRead + std::fmt::Debug,
{
    let mut buf = vec![];
    loop {
        let mut byte = [0u8];
        let nread = r.read(&mut byte).await?;
        if nread == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "EOF while reading leb128 encoded value",
            )
            .into());
        }
        buf.push(byte[0]);

        match leb128::read::unsigned(&mut buf.as_slice()) {
            Ok(n) => {
                return Ok(n);
            }
            Err(leb128::read::Error::IoError(_)) => continue,
            Err(leb128::read::Error::Overflow) => anyhow::bail!("leb128 is too large"),
        }
    }
}

/// Read a single leb128 encoded value from the stream
fn read_u64<R: std::io::Read>(mut r: R) -> anyhow::Result<u64> {
    leb128::read::unsigned(&mut r)
        .map_err(|err| match err {
            leb128::read::Error::IoError(ioerr) => anyhow::Error::new(ioerr),
            err => anyhow::Error::new(err),
        })
        .context("reading leb128")
}

#[derive(Debug)]
struct Decoded {
    ident: u64,
    serial: u64,
    data: Vec<u8>,
    is_compressed: bool,
}

/// Decode a frame.
/// See encode_raw() for the frame format.
async fn decode_raw_async<R: Unpin + AsyncRead + std::fmt::Debug>(
    r: &mut R,
    max_serial: Option<u64>,
) -> anyhow::Result<Decoded> {
    let len = read_u64_async(r)
        .await
        .context("decode_raw_async failed to read PDU length")?;
    let (len, is_compressed) = if (len & COMPRESSED_MASK) != 0 {
        (len & !COMPRESSED_MASK, true)
    } else {
        (len, false)
    };
    let serial = read_u64_async(r)
        .await
        .context("decode_raw_async failed to read PDU serial")?;
    if let Some(max_serial) = max_serial {
        if serial > max_serial && max_serial > 0 {
            return Err(CorruptResponse(format!(
                "decode_raw_async: serial {serial} is implausibly large \
                (bigger than {max_serial})"
            ))
            .into());
        }
    }
    let ident = read_u64_async(r)
        .await
        .context("decode_raw_async failed to read PDU ident")?;
    let data_len =
        match (len as usize).overflowing_sub(encoded_length(ident) + encoded_length(serial)) {
            (_, true) => {
                return Err(CorruptResponse(format!(
                    "decode_raw_async: sizes don't make sense: \
                    len:{len} serial:{serial} (enc={}) ident:{ident} (enc={})",
                    encoded_length(serial),
                    encoded_length(ident)
                ))
                .into());
            }
            (data_len, false) => data_len,
        };

    if is_compressed {
        metrics::histogram!("pdu.decode.compressed.size").record(data_len as f64);
    } else {
        metrics::histogram!("pdu.decode.size").record(data_len as f64);
    }

    let mut data = vec![0u8; data_len];
    r.read_exact(&mut data).await.with_context(|| {
        format!(
            "decode_raw_async failed to read {} bytes of data \
            for PDU of length {} with serial={} ident={}",
            data_len, len, serial, ident
        )
    })?;
    Ok(Decoded {
        ident,
        serial,
        data,
        is_compressed,
    })
}

/// LBTerm: how much a `PduReader` reads at a time, and so the most it holds
const PDU_READ_CHUNK: usize = 64 * 1024;

/// The most a frame's header (length, serial, ident) can take, at 10 bytes a
/// leb128 u64
const MAX_FRAME_HEADER: usize = 30;

/// A frame's header, and how many bytes it took
#[derive(Debug, PartialEq)]
struct FrameHeader {
    header_len: usize,
    ident: u64,
    serial: u64,
    data_len: usize,
    is_compressed: bool,
}

/// One leb128 value off the front of `cursor`: None while the bytes there
/// end before it does
fn take_leb128(cursor: &mut &[u8]) -> anyhow::Result<Option<u64>> {
    match leb128::read::unsigned(cursor) {
        Ok(n) => Ok(Some(n)),
        Err(leb128::read::Error::IoError(_)) => Ok(None),
        Err(leb128::read::Error::Overflow) => anyhow::bail!("leb128 is too large"),
    }
}

/// The header at the start of `bytes`, as decode_raw_async reads it, with
/// its checks; None while `bytes` end before it does
fn parse_frame_header(bytes: &[u8], max_serial: Option<u64>) -> anyhow::Result<Option<FrameHeader>> {
    let mut cursor = bytes;
    let Some(len) = take_leb128(&mut cursor).context("reading PDU length")? else {
        return Ok(None);
    };
    let (len, is_compressed) = if (len & COMPRESSED_MASK) != 0 {
        (len & !COMPRESSED_MASK, true)
    } else {
        (len, false)
    };
    let Some(serial) = take_leb128(&mut cursor).context("reading PDU serial")? else {
        return Ok(None);
    };
    if let Some(max_serial) = max_serial {
        if serial > max_serial && max_serial > 0 {
            return Err(CorruptResponse(format!(
                "decode_raw_async: serial {serial} is implausibly large \
                (bigger than {max_serial})"
            ))
            .into());
        }
    }
    let Some(ident) = take_leb128(&mut cursor).context("reading PDU ident")? else {
        return Ok(None);
    };
    let data_len =
        match (len as usize).overflowing_sub(encoded_length(ident) + encoded_length(serial)) {
            (_, true) => {
                return Err(CorruptResponse(format!(
                    "decode_raw_async: sizes don't make sense: \
                    len:{len} serial:{serial} (enc={}) ident:{ident} (enc={})",
                    encoded_length(serial),
                    encoded_length(ident)
                ))
                .into());
            }
            (data_len, false) => data_len,
        };
    Ok(Some(FrameHeader {
        header_len: bytes.len() - cursor.len(),
        ident,
        serial,
        data_len,
        is_compressed,
    }))
}

/// LBTerm: a connection's bytes read but not yet decoded, so frames come off
/// it a read at a time rather than a byte at a time. Upstream's
/// `decode_raw_async` read each header's length, serial and ident a byte at
/// a time from the unbuffered socket, each byte a recv of its own (and a Vec
/// pushed and parsed again), then the data with another: four to eight recvs
/// a PDU, on the mux server's main thread and the GUI's client thread. This
/// reads up to `PDU_READ_CHUNK` at once and takes the frames out of that; a
/// frame bigger than it reads its data straight from the stream. The bytes
/// on the wire are the same.
///
/// A socket's readiness says nothing of bytes already read into here, so a
/// loop that waits for the socket to be readable before decoding must not
/// wait while `has_buffered()` (the server's dispatch and the client thread
/// go straight to the next decode).
#[derive(Debug, Default)]
pub struct PduReader {
    buf: Vec<u8>,
    start: usize,
    end: usize,
}

impl PduReader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bytes read and not yet decoded, which the next decode takes first
    pub fn has_buffered(&self) -> bool {
        self.start < self.end
    }

    /// One read from `r` added to what's here, for a caller waiting on `need`
    /// bytes from `start` (at most `PDU_READ_CHUNK`): what's here moves to
    /// the front only when they wouldn't fit after it. An EOF is
    /// UnexpectedEof, as upstream's reads of a header or data took it
    async fn fill<R: Unpin + AsyncRead>(&mut self, r: &mut R, need: usize) -> std::io::Result<()> {
        if self.buf.is_empty() {
            self.buf = vec![0u8; PDU_READ_CHUNK];
        }
        if self.start == self.end {
            self.start = 0;
            self.end = 0;
        } else if self.start + need > self.buf.len() {
            self.buf.copy_within(self.start..self.end, 0);
            self.end -= self.start;
            self.start = 0;
        }
        // Fewer than `need` bytes are here and they fit from `start`, so
        // there's room, unless a header's leb128 never ends (upstream's read
        // went on a byte at a time until one did)
        if self.end == self.buf.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("a PDU's header runs past {PDU_READ_CHUNK} bytes"),
            ));
        }
        let nread = r.read(&mut self.buf[self.end..]).await?;
        if nread == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "EOF while reading a PDU",
            ));
        }
        self.end += nread;
        Ok(())
    }

    /// The next frame, from what's here and as few reads of `r` as it needs
    async fn decode_frame_async<R: Unpin + AsyncRead>(
        &mut self,
        r: &mut R,
        max_serial: Option<u64>,
    ) -> anyhow::Result<DecodedPdu> {
        let header = loop {
            if let Some(header) = parse_frame_header(&self.buf[self.start..self.end], max_serial)
                .context("decode_raw_async failed to read a PDU's header")?
            {
                break header;
            }
            self.fill(r, MAX_FRAME_HEADER)
                .await
                .context("decode_raw_async failed to read a PDU's header")?;
        };
        self.start += header.header_len;
        let FrameHeader {
            ident,
            serial,
            data_len,
            is_compressed,
            ..
        } = header;

        if is_compressed {
            metrics::histogram!("pdu.decode.compressed.size").record(data_len as f64);
        } else {
            metrics::histogram!("pdu.decode.size").record(data_len as f64);
        }

        let read_context = || {
            format!(
                "decode_raw_async failed to read {data_len} bytes of data \
                for PDU with serial={serial} ident={ident}"
            )
        };
        if data_len <= PDU_READ_CHUNK - MAX_FRAME_HEADER {
            while self.end - self.start < data_len {
                self.fill(r, data_len).await.with_context(read_context)?;
            }
        }
        if self.end - self.start >= data_len {
            let data = &self.buf[self.start..self.start + data_len];
            let decoded = Pdu::decode_frame(ident, serial, data, is_compressed);
            self.start += data_len;
            return decoded;
        }

        // Bigger than a read: what's here, then the rest straight from `r`
        let mut data = vec![0u8; data_len];
        let have = self.end - self.start;
        data[..have].copy_from_slice(&self.buf[self.start..self.end]);
        self.start = self.end;
        r.read_exact(&mut data[have..])
            .await
            .with_context(read_context)?;
        Pdu::decode_frame(ident, serial, &data, is_compressed)
    }

    /// Whether a PDU's bytes are waiting: here already, or on `r` for a read that doesn't wait,
    /// whose bytes are kept for the next decode. A read that fails says so too, for the decode
    /// to meet the error itself
    pub fn read_now<R: Unpin + AsyncRead>(&mut self, r: &mut R) -> bool {
        if self.has_buffered() {
            return true;
        }
        let mut read = std::pin::pin!(self.fill(r, MAX_FRAME_HEADER));
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        read.as_mut().poll(&mut cx).is_ready()
    }

    /// Decode the next PDU: Pdu::decode_async, reading ahead
    pub async fn decode_async<R: Unpin + AsyncRead>(
        &mut self,
        r: &mut R,
        max_serial: Option<u64>,
    ) -> anyhow::Result<DecodedPdu> {
        self.decode_frame_async(r, max_serial)
            .await
            .context("decoding a PDU")
    }
}

/// Decode a frame.
/// See encode_raw() for the frame format.
fn decode_raw<R: std::io::Read>(mut r: R) -> anyhow::Result<Decoded> {
    let len = read_u64(r.by_ref()).context("reading PDU length")?;
    let (len, is_compressed) = if (len & COMPRESSED_MASK) != 0 {
        (len & !COMPRESSED_MASK, true)
    } else {
        (len, false)
    };
    let serial = read_u64(r.by_ref()).context("reading PDU serial")?;
    let ident = read_u64(r.by_ref()).context("reading PDU ident")?;
    let data_len =
        match (len as usize).overflowing_sub(encoded_length(ident) + encoded_length(serial)) {
            (_, true) => {
                anyhow::bail!(
                    "sizes don't make sense: len:{} serial:{} (enc={}) ident:{} (enc={})",
                    len,
                    serial,
                    encoded_length(serial),
                    ident,
                    encoded_length(ident)
                );
            }
            (data_len, false) => data_len,
        };

    if is_compressed {
        metrics::histogram!("pdu.decode.compressed.size").record(data_len as f64);
    } else {
        metrics::histogram!("pdu.decode.size").record(data_len as f64);
    }

    let mut data = vec![0u8; data_len];
    r.read_exact(&mut data).with_context(|| {
        format!(
            "reading {} bytes of data for PDU of length {} with serial={} ident={}",
            data_len, len, serial, ident
        )
    })?;
    Ok(Decoded {
        ident,
        serial,
        data,
        is_compressed,
    })
}

#[derive(Debug, PartialEq)]
pub struct DecodedPdu {
    pub serial: u64,
    pub pdu: Pdu,
}

/// If the serialized size is larger than this, then we'll consider compressing it
const COMPRESS_THRESH: usize = 32;

fn serialize<T: serde::Serialize>(t: &T) -> Result<(Vec<u8>, bool), Error> {
    serialize_with(t, compress)
}

/// LBTerm: compression only saves bytes, so a PDU that fails to compress goes
/// out uncompressed, as one that doesn't compress well already does. zstd
/// allocated a few MB of workspace for each PDU it compressed until its
/// contexts were kept (`KeptZstd`). On 2026-10-21 the PC ran out of commit
/// (two image viewers took 33 GB), that allocation failed in the mux server
/// ("Allocation error : not enough memory"), and the server dropped the GUI's
/// connection over it. A unix-domain client doesn't reconnect, so every tab
/// left the window.
///
/// LBTerm: `compress` is given the bytes already serialized. Upstream's
/// serialized the PDU a second time into its encoder: the same bytes, made
/// twice.
fn serialize_with<T: serde::Serialize>(
    t: &T,
    compress: impl FnOnce(&[u8]) -> Result<Vec<u8>, Error>,
) -> Result<(Vec<u8>, bool), Error> {
    let mut uncompressed = Vec::new();
    let mut encode = varbincode::Serializer::new(&mut uncompressed);
    t.serialize(&mut encode)?;

    if uncompressed.len() <= COMPRESS_THRESH {
        return Ok((uncompressed, false));
    }
    // It's a little heavy; let's try compressing it
    let compressed = match compress(&uncompressed) {
        Ok(compressed) => compressed,
        Err(err) => {
            log::warn!(
                "compressing a PDU of {} bytes failed, sending it uncompressed: {:#}",
                uncompressed.len(),
                err
            );
            return Ok((uncompressed, false));
        }
    };

    log::debug!(
        "serialized+compress len {} vs {}",
        compressed.len(),
        uncompressed.len()
    );

    if compressed.len() < uncompressed.len() {
        Ok((compressed, true))
    } else {
        Ok((uncompressed, false))
    }
}

fn compress(data: &[u8]) -> Result<Vec<u8>, Error> {
    KEPT_ZSTD
        .try_with(|kept| kept.borrow_mut().compress(data))
        .map_err(|_| anyhow::anyhow!("this thread's zstd contexts are gone (it's ending)"))?
}

fn deserialize<T: serde::de::DeserializeOwned>(data: &[u8], is_compressed: bool) -> Result<T, Error> {
    if is_compressed {
        KEPT_ZSTD
            .try_with(|kept| kept.borrow_mut().decompress(data))
            .map_err(|_| anyhow::anyhow!("this thread's zstd contexts are gone (it's ending)"))?
    } else {
        let mut data = data;
        let mut decode = varbincode::Deserializer::new(&mut data);
        serde::Deserialize::deserialize(&mut decode).map_err(Into::into)
    }
}

thread_local! {
    static KEPT_ZSTD: std::cell::RefCell<KeptZstd> = std::cell::RefCell::new(KeptZstd::default());
}

/// LBTerm: a decompressed PDU's bytes are kept for the next one while they
/// take no more than this; a bigger one's go when it's decoded
const KEEP_DECOMPRESSED: usize = 1 << 20;

/// LBTerm: the zstd contexts a thread compresses and decompresses PDUs with,
/// made the first time it needs each and kept. Upstream made new ones for
/// every PDU over `COMPRESS_THRESH`, on the mux server's only thread and the
/// GUI's client thread: a streaming encoder, whose input size zstd isn't told,
/// so it sets up level 3 for any size (a 2 MB window and 768 KB of match
/// tables, about 3 MB, the tables cleared), to compress a push of a few
/// hundred bytes; and a decoder that allocated the frame's 2 MB window to read
/// it. Allocations that size are mapped and unmapped each time, with a page
/// fault for each page the tables' clearing touches.
///
/// A kept encoder is reset for each PDU (`ZSTD_reset_session_only`) and given
/// its bytes as upstream's was, `ZSTD_compressStream` then `ZSTD_endStream`
/// with the size untold, so each PDU is still one frame of its own, with the
/// same header and parameters and the same bytes on the wire (the tests
/// compare them with upstream's path, PDU by PDU, in one thread as the server
/// sends them); zstd keeps its workspace from frame to frame, and its match
/// tables' index runs on, so nothing in them needs clearing. A decoder reset
/// for each frame keeps its window.
#[derive(Default)]
struct KeptZstd {
    encoder: Option<zstd::stream::raw::Encoder<'static>>,
    decoder: Option<zstd::stream::raw::Decoder<'static>>,
    decompressed: Vec<u8>,
    /// How many encoders and decoders this thread has made, for the tests
    encoders_made: usize,
    decoders_made: usize,
}

impl KeptZstd {
    fn compress(&mut self, data: &[u8]) -> Result<Vec<u8>, Error> {
        let result = self.compress_frame(data);
        if result.is_err() {
            // A context zstd failed in (out of memory, as on 2026-10-21) isn't
            // used again: the next PDU makes a new one
            self.encoder = None;
        }
        result
    }

    fn compress_frame(&mut self, data: &[u8]) -> Result<Vec<u8>, Error> {
        use zstd::stream::raw::{Encoder, InBuffer, Operation};
        match &mut self.encoder {
            Some(encoder) => encoder.reinit()?,
            None => {
                self.encoders_made += 1;
                self.encoder = Some(Encoder::new(zstd::DEFAULT_COMPRESSION_LEVEL)?);
            }
        }
        let encoder = self.encoder.as_mut().expect("made above");
        let mut compressed =
            Vec::with_capacity(zstd::zstd_safe::compress_bound(data.len().min(1 << 20)));
        let mut input = InBuffer::around(data);
        while input.pos() < data.len() {
            encoder.run(&mut input, &mut out_buffer(&mut compressed))?;
        }
        while encoder.finish(&mut out_buffer(&mut compressed), true)? != 0 {}
        Ok(compressed)
    }

    fn decompress<T: serde::de::DeserializeOwned>(&mut self, data: &[u8]) -> Result<T, Error> {
        let result = self.decompress_frame(data);
        if result.is_err() {
            self.decoder = None;
        }
        result?;
        let mut decompressed = self.decompressed.as_slice();
        let mut decode = varbincode::Deserializer::new(&mut decompressed);
        let decoded = serde::Deserialize::deserialize(&mut decode).map_err(Into::into);
        if self.decompressed.capacity() > KEEP_DECOMPRESSED {
            self.decompressed = Vec::new();
        }
        decoded
    }

    /// The frame in `data` into `self.decompressed`
    fn decompress_frame(&mut self, data: &[u8]) -> Result<(), Error> {
        use zstd::stream::raw::{Decoder, InBuffer, Operation};
        match &mut self.decoder {
            Some(decoder) => decoder.reinit()?,
            None => {
                self.decoders_made += 1;
                self.decoder = Some(Decoder::new()?);
            }
        }
        let decoder = self.decoder.as_mut().expect("made above");
        let out = &mut self.decompressed;
        out.clear();
        out.reserve(data.len().saturating_mul(2));
        let mut input = InBuffer::around(data);
        loop {
            let before = (input.pos(), out.len());
            // 0 once the frame is decoded and all of it given out
            if decoder.run(&mut input, &mut out_buffer(out))? == 0 {
                return Ok(());
            }
            if (input.pos(), out.len()) == before {
                anyhow::bail!(
                    "a compressed PDU of {} bytes ends before its zstd frame does",
                    data.len()
                );
            }
        }
    }
}

/// zstd's view of the room left at the end of `out`, which doubles once
/// there's little left
fn out_buffer(out: &mut Vec<u8>) -> zstd::stream::raw::OutBuffer<'_, Vec<u8>> {
    if out.capacity() - out.len() < 256 {
        out.reserve(out.capacity().max(4096));
    }
    let pos = out.len();
    zstd::stream::raw::OutBuffer::around_pos(out, pos)
}

macro_rules! pdu {
    ($( $name:ident:$vers:expr),* $(,)?) => {
        #[derive(PartialEq, Debug)]
        pub enum Pdu {
            Invalid{ident: u64},
            $(
                $name($name)
            ,)*
        }

        impl Pdu {
            pub fn encode<W: std::io::Write>(&self, w: W, serial: u64) -> Result<(), Error> {
                match self {
                    Pdu::Invalid{..} => bail!("attempted to serialize Pdu::Invalid"),
                    $(
                        Pdu::$name(s) => {
                            let (data, is_compressed) = serialize(s)?;
                            lbterm_note_size(data.len());
                            let encoded_size = encode_raw($vers, serial, &data, is_compressed, w)?;
                            log::debug!("encode {} size={encoded_size}", stringify!($name));
                            metrics::histogram!("pdu.size", "pdu" => stringify!($name)).record(encoded_size as f64);
                            metrics::histogram!("pdu.size.rate", "pdu" => stringify!($name)).record(encoded_size as f64);
                            Ok(())
                        }
                    ,)*
                }
            }

            pub async fn encode_async<W: Unpin + AsyncWriteExt>(&self, w: &mut W, serial: u64) -> Result<(), Error> {
                match self {
                    Pdu::Invalid{..} => bail!("attempted to serialize Pdu::Invalid"),
                    $(
                        Pdu::$name(s) => {
                            let (data, is_compressed) = serialize(s)?;
                            lbterm_note_size(data.len());
                            let encoded_size = encode_raw_async($vers, serial, &data, is_compressed, w).await?;
                            log::debug!("encode_async {} size={encoded_size}", stringify!($name));
                            metrics::histogram!("pdu.size", "pdu" => stringify!($name)).record(encoded_size as f64);
                            metrics::histogram!("pdu.size.rate", "pdu" => stringify!($name)).record(encoded_size as f64);
                            Ok(())
                        }
                    ,)*
                }
            }

            /// LBTerm: the PDU in a frame's data, for PduReader, as decode_async
            /// makes it from its frame
            fn decode_frame(ident: u64, serial: u64, data: &[u8], is_compressed: bool) -> Result<DecodedPdu, Error> {
                lbterm_note_size(data.len());
                match ident {
                    $(
                        $vers => {
                            metrics::histogram!("pdu.size", "pdu" => stringify!($name)).record(data.len() as f64);
                            Ok(DecodedPdu {
                                serial,
                                pdu: Pdu::$name(deserialize(data, is_compressed)?)
                            })
                        }
                    ,)*
                    _ => {
                        metrics::histogram!("pdu.size", "pdu" => "??").record(data.len() as f64);
                        Ok(DecodedPdu {
                            serial,
                            pdu: Pdu::Invalid{ident}
                        })
                    }
                }
            }

            pub fn pdu_name(&self) -> &'static str {
                match self {
                    Pdu::Invalid{..} => "Invalid",
                    $(
                        Pdu::$name(_) => {
                            stringify!($name)
                        }
                    ,)*
                }
            }

            pub fn decode<R: std::io::Read>(r: R) -> Result<DecodedPdu, Error> {
                let decoded = decode_raw(r).context("decoding a PDU")?;
                lbterm_note_size(decoded.data.len());
                match decoded.ident {
                    $(
                        $vers => {
                            metrics::histogram!("pdu.size", "pdu" => stringify!($name)).record(decoded.data.len() as f64);
                            metrics::histogram!("pdu.size.rate", "pdu" => stringify!($name)).record(decoded.data.len() as f64);
                            Ok(DecodedPdu {
                                serial: decoded.serial,
                                pdu: Pdu::$name(deserialize(decoded.data.as_slice(), decoded.is_compressed)?)
                            })
                        }
                    ,)*
                    _ => {
                        metrics::histogram!("pdu.size", "pdu" => "??").record(decoded.data.len() as f64);
                        metrics::histogram!("pdu.size.rate", "pdu" => "??").record(decoded.data.len() as f64);
                        Ok(DecodedPdu {
                            serial: decoded.serial,
                            pdu: Pdu::Invalid{ident:decoded.ident}
                        })
                    }
                }
            }

            pub async fn decode_async<R>(r: &mut R, max_serial: Option<u64>) -> Result<DecodedPdu, Error>
                where R: std::marker::Unpin,
                      R: AsyncRead,
                      R: std::fmt::Debug
            {
                let decoded = decode_raw_async(r, max_serial).await.context("decoding a PDU")?;
                lbterm_note_size(decoded.data.len());
                match decoded.ident {
                    $(
                        $vers => {
                            metrics::histogram!("pdu.size", "pdu" => stringify!($name)).record(decoded.data.len() as f64);
                            Ok(DecodedPdu {
                                serial: decoded.serial,
                                pdu: Pdu::$name(deserialize(decoded.data.as_slice(), decoded.is_compressed)?)
                            })
                        }
                    ,)*
                    _ => {
                        metrics::histogram!("pdu.size", "pdu" => "??").record(decoded.data.len() as f64);
                        Ok(DecodedPdu {
                            serial: decoded.serial,
                            pdu: Pdu::Invalid{ident:decoded.ident}
                        })
                    }
                }
            }
        }
    }
}

/// The overall version of the codec.
/// This must be bumped when backwards incompatible changes
/// are made to the types and protocol.
pub const CODEC_VERSION: usize = 46;

// Defines the Pdu enum.
// Each struct has an explicit identifying number.
// This allows removal of obsolete structs,
// and defining newer structs as the protocol evolves.
pdu! {
    ErrorResponse: 0,
    Ping: 1,
    Pong: 2,
    ListPanes: 3,
    ListPanesResponse: 4,
    SpawnResponse: 8,
    WriteToPane: 9,
    UnitResponse: 10,
    SendKeyDown: 11,
    SendMouseEvent: 12,
    SendPaste: 13,
    Resize: 14,
    SetClipboard: 20,
    GetLines: 22,
    GetLinesResponse: 23,
    GetPaneRenderChanges: 24,
    GetPaneRenderChangesResponse: 25,
    GetCodecVersion: 26,
    GetCodecVersionResponse: 27,
    GetTlsCreds: 28,
    GetTlsCredsResponse: 29,
    LivenessResponse: 30,
    SearchScrollbackRequest: 31,
    SearchScrollbackResponse: 32,
    SetPaneZoomed: 33,
    SplitPane: 34,
    KillPane: 35,
    SpawnV2: 36,
    PaneRemoved: 37,
    SetPalette: 38,
    NotifyAlert: 39,
    SetClientId: 40,
    GetClientList: 41,
    GetClientListResponse: 42,
    SetWindowWorkspace: 43,
    WindowWorkspaceChanged: 44,
    SetFocusedPane: 45,
    GetImageCell: 46,
    GetImageCellResponse: 47,
    MovePaneToNewTab: 48,
    MovePaneToNewTabResponse: 49,
    ActivatePaneDirection: 50,
    GetPaneRenderableDimensions: 51,
    GetPaneRenderableDimensionsResponse: 52,
    PaneFocused: 53,
    TabResized: 54,
    TabAddedToWindow: 55,
    TabTitleChanged: 56,
    WindowTitleChanged: 57,
    RenameWorkspace: 58,
    EraseScrollbackRequest: 59,
    GetPaneDirection: 60,
    GetPaneDirectionResponse: 61,
    AdjustPaneSize: 62,
    // LBTerm's own, numbered well clear of upstream's. Additive, so the
    // codec version stays upstream's: a peer that doesn't know one decodes
    // it as Pdu::Invalid. A server answers an Invalid request with
    // ErrorResponse and goes on; a window ends the connection on an Invalid
    // unilateral PDU (wezterm-client's process_unilateral), so the server
    // sends a window one of these only once the window has shown it knows
    // it (lbterm/PITFALLS.md, 188). A notice from a window that waits for no
    // answer (PanesDrawn) goes only to a server known to read it: an older
    // one's ErrorResponse, with no promise waiting, ends the connection (227).
    SetProgramStatus: 7501,
    SpawnNoActivate: 7502,
    GetPromptRows: 7503,
    GetPromptRowsResponse: 7504,
    SetShownPanes: 7505,
    PaneHeld: 7506,
    PanesDrawn: 7507,
    GetParkedTabs: 7508,
    ParkedTabs: 7509,
    TabParked: 7510,
}

impl Pdu {
    /// Returns true if this type of Pdu represents action taken
    /// directly by a user, rather than background traffic on
    /// a live connection
    pub fn is_user_input(&self) -> bool {
        match self {
            Self::WriteToPane(_)
            | Self::SendKeyDown(_)
            | Self::SendMouseEvent(_)
            | Self::SendPaste(_)
            | Self::Resize(_)
            | Self::SetClipboard(_)
            | Self::SetPaneZoomed(_)
            | Self::SpawnV2(_) => true,
            _ => false,
        }
    }

    pub fn stream_decode(buffer: &mut Vec<u8>) -> anyhow::Result<Option<DecodedPdu>> {
        let mut cursor = Cursor::new(buffer.as_slice());
        match Self::decode(&mut cursor) {
            Ok(decoded) => {
                let consumed = cursor.position() as usize;
                let remain = buffer.len() - consumed;
                // Remove `consumed` bytes from the start of the vec.
                // This is safe because the vec is just bytes and we are
                // constrained the offsets accordingly.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        buffer.as_ptr().add(consumed),
                        buffer.as_mut_ptr(),
                        remain,
                    );
                }
                buffer.truncate(remain);
                Ok(Some(decoded))
            }
            Err(err) => {
                if let Some(ioerr) = err.root_cause().downcast_ref::<std::io::Error>() {
                    match ioerr.kind() {
                        std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::WouldBlock => {
                            return Ok(None);
                        }
                        _ => {}
                    }
                } else {
                    log::error!("not an ioerror in stream_decode: {:?}", err);
                }
                Err(err)
            }
        }
    }

    pub fn try_read_and_decode<R: std::io::Read>(
        r: &mut R,
        buffer: &mut Vec<u8>,
    ) -> anyhow::Result<Option<DecodedPdu>> {
        loop {
            if let Some(decoded) =
                Self::stream_decode(buffer).context("stream_decode of buffer for PDU")?
            {
                return Ok(Some(decoded));
            }

            let mut buf = [0u8; 4096];
            let size = match r.read(&mut buf) {
                Ok(size) => size,
                Err(err) => {
                    if err.kind() == std::io::ErrorKind::WouldBlock {
                        return Ok(None);
                    }
                    return Err(err.into());
                }
            };
            if size == 0 {
                return Err(
                    std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "End Of File").into(),
                );
            }

            buffer.extend_from_slice(&buf[0..size]);
        }
    }

    pub fn pane_id(&self) -> Option<PaneId> {
        match self {
            Pdu::GetPaneRenderChangesResponse(GetPaneRenderChangesResponse { pane_id, .. })
            | Pdu::SetPalette(SetPalette { pane_id, .. })
            | Pdu::NotifyAlert(NotifyAlert { pane_id, .. })
            | Pdu::SetProgramStatus(SetProgramStatus { pane_id, .. })
            | Pdu::SetClipboard(SetClipboard { pane_id, .. })
            | Pdu::PaneFocused(PaneFocused { pane_id })
            | Pdu::PaneHeld(PaneHeld { pane_id, .. })
            | Pdu::PaneRemoved(PaneRemoved { pane_id }) => Some(*pane_id),
            _ => None,
        }
    }
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct UnitResponse {}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct ErrorResponse {
    pub reason: String,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetCodecVersion {}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetCodecVersionResponse {
    pub codec_vers: usize,
    pub version_string: String,
    pub executable_path: PathBuf,
    pub config_file_path: Option<PathBuf>,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct Ping {}
#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct Pong {}

/// Requests a client certificate to authenticate against
/// the TLS based server
#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetTlsCreds {}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetTlsCredsResponse {
    /// The signing certificate
    pub ca_cert_pem: String,
    /// A client authentication certificate and private
    /// key, PEM encoded
    pub client_cert_pem: String,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct ListPanes {}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct ListPanesResponse {
    pub tabs: Vec<PaneNode>,
    pub tab_titles: Vec<String>,
    pub window_titles: HashMap<WindowId, String>,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SplitPane {
    pub pane_id: PaneId,
    pub split_request: SplitRequest,
    pub command: Option<CommandBuilder>,
    pub command_dir: Option<String>,
    pub domain: config::keyassignment::SpawnTabDomain,
    /// Instead of spawning a command, move the specified
    /// pane into the new split target
    pub move_pane_id: Option<PaneId>,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct MovePaneToNewTab {
    pub pane_id: PaneId,
    pub window_id: Option<WindowId>,
    pub workspace_for_new_window: Option<String>,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct MovePaneToNewTabResponse {
    pub tab_id: TabId,
    pub window_id: WindowId,
}

#[derive(Deserialize, Serialize, PartialEq, Debug, Clone)]
pub struct SpawnV2 {
    pub domain: config::keyassignment::SpawnTabDomain,
    /// If None, create a new window for this new tab
    pub window_id: Option<WindowId>,
    pub command: Option<CommandBuilder>,
    pub command_dir: Option<String>,
    pub size: TerminalSize,
    pub workspace: String,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct PaneRemoved {
    pub pane_id: PaneId,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct KillPane {
    pub pane_id: PaneId,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SpawnResponse {
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub window_id: WindowId,
    pub size: TerminalSize,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct WriteToPane {
    pub pane_id: PaneId,
    pub data: Vec<u8>,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SendPaste {
    pub pane_id: PaneId,
    pub data: String,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SendKeyDown {
    pub pane_id: TabId,
    pub event: termwiz::input::KeyEvent,
    pub input_serial: InputSerial,
}

/// InputSerial is used to sequence input requests with output events.
/// It started life as a monotonic sequence number but evolved into
/// the number of milliseconds since the unix epoch.
#[derive(Deserialize, Serialize, PartialEq, Eq, Debug, Clone, Copy, PartialOrd, Ord)]
pub struct InputSerial(u64);

impl InputSerial {
    pub const fn empty() -> Self {
        Self(0)
    }

    pub fn now() -> Self {
        std::time::SystemTime::now().into()
    }

    pub fn elapsed_millis(&self) -> u64 {
        let now = InputSerial::now();
        now.0 - self.0
    }
}

impl From<std::time::SystemTime> for InputSerial {
    fn from(val: std::time::SystemTime) -> Self {
        let duration = val
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .expect("SystemTime before unix epoch?");
        let millis: u64 = duration
            .as_millis()
            .try_into()
            .expect("millisecond count to fit in u64");
        InputSerial(millis)
    }
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SendMouseEvent {
    pub pane_id: PaneId,
    pub event: wezterm_term::input::MouseEvent,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SetClipboard {
    pub pane_id: PaneId,
    pub clipboard: Option<String>,
    pub selection: ClipboardSelection,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SetWindowWorkspace {
    pub window_id: WindowId,
    pub workspace: String,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct RenameWorkspace {
    pub old_workspace: String,
    pub new_workspace: String,
}

/// This is used both as a notification from server->client
/// and as a configuration request from client->server when
/// the client's preferred configuration changes
#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SetPalette {
    pub pane_id: PaneId,
    pub palette: ColorPalette,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct NotifyAlert {
    pub pane_id: PaneId,
    pub alert: Alert,
}

/// LBTerm: server to client, a pane's OSC 7501 records, all of them, sent
/// when they change and when a client first looks at the pane.
/// Alert::ProgramStatus only says that they changed.
#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SetProgramStatus {
    pub pane_id: PaneId,
    pub records: Vec<wezterm_term::program_status::ProgramStatusRecord>,
}

/// LBTerm: SpawnV2, leaving the window on the tab it was on
/// (`lbterm cli spawn --no-activate`). A PDU of its own rather than a field
/// on SpawnV2, so a server of an older build still reads every other spawn.
/// Answered with SpawnResponse. Not user input: nobody pressed anything.
#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SpawnNoActivate {
    pub spawn: SpawnV2,
}

/// LBTerm: window to server, the panes the window shows (lbterm/MUX.md, "Tabs out of view:
/// output held at the server"): the panes of each of its mux windows' active tab, a zoomed tab's
/// zoomed pane alone, numbered as the server numbers them. Each says the whole set, which
/// replaces the one before, so a lost or repeated one leaves nothing half said. A server that has
/// been told sends the lines of a pane out of the set only when it's shown, quiet or asked for
/// (its alerts and title still go at once), and a server never told (by an older window, or the
/// cli) pushes every pane as it changes. Answered with UnitResponse; a server of an older build
/// answers ErrorResponse naming it, as for any PDU it doesn't know (`lacks_shown_panes`), and the
/// window says no more on that connection. It's also the window saying it knows [`PaneHeld`],
/// which the server sends only to a window that has said this
#[derive(Deserialize, Serialize, PartialEq, Eq, Debug, Clone, Default)]
pub struct SetShownPanes {
    pub panes: Vec<ShownPane>,
}

/// LBTerm: a pane a window shows, in [`SetShownPanes`]
#[derive(Deserialize, Serialize, PartialEq, Eq, Debug, Clone, Copy)]
pub struct ShownPane {
    pub pane_id: PaneId,
    /// Always None, and read by no server: a frame's word that it read a push goes in
    /// [`PanesDrawn`], a notice of its own, since this PDU is a request whose answer a window
    /// waits for (lbterm/MUX.md, "A shown pane's pace"). Kept because a field can't be taken out
    /// of a struct a peer decodes: varbincode reads fields in order, with no names
    pub drawn: Option<SequenceNo>,
}

/// LBTerm: window to server, a notice: the newest push of each pane named that the window had
/// applied when a frame of it read the pane (or, in a window that can't be seen, as it applied
/// it), so a server pacing a shown pane can send it the next (lbterm/MUX.md, "A shown pane's
/// pace"; lbterm/FLOWS.md, pane output, What could go 12). One a paint of a window and server,
/// naming each pane the frame read with a push of it applied since it last said it (the same
/// seqno again included: a round push of a title alone keeps the seqno), and one a push for a
/// pane it shows that no frame will draw soon (wezterm-client's lbterm_drawn.rs). Sent at serial
/// 0 and answered with nothing, and only to a server that has answered [`SetShownPanes`] with
/// UnitResponse, which lands with it: a server that doesn't know it answers ErrorResponse, and a
/// window's client thread ends the connection on an answer it holds no promise for
/// (lbterm/PITFALLS.md, 227). A server that's never told paces nothing
#[derive(Deserialize, Serialize, PartialEq, Eq, Debug, Clone, Default)]
pub struct PanesDrawn {
    pub panes: Vec<PaneDrawn>,
}

/// LBTerm: a pane in [`PanesDrawn`]: its id as the server numbers it, and the seqno of the push
/// the frame read (the `seqno` of the newest GetPaneRenderChangesResponse of it applied)
#[derive(Deserialize, Serialize, PartialEq, Eq, Debug, Clone, Copy)]
pub struct PaneDrawn {
    pub pane_id: PaneId,
    pub seqno: SequenceNo,
}

/// Whether `error`, the answer to a [`SetShownPanes`], is a server's that doesn't know the PDU
/// (an older build: `Pdu::Invalid` answered with ErrorResponse by its sessionhandler), rather
/// than any other failure, after which the window still says what it shows
pub fn lacks_shown_panes(error: &str) -> bool {
    error.contains(&format!("Invalid {{ ident: {} }}", SET_SHOWN_PANES_IDENT))
}

/// [`SetShownPanes`]'s ident on the wire, as the `pdu!` table numbers it
pub const SET_SHOWN_PANES_IDENT: u64 = 7505;

/// LBTerm: server to window, unilateral: the server holds `pane_id`'s lines, which the window
/// has as of its push of `seqno`, because the window doesn't show it (lbterm/MUX.md, "Tabs out
/// of view: output held at the server"). Sent once a hold, at the pane's first output after a
/// push that carried its lines, and only to a window that has said what it shows
/// ([`SetShownPanes`]), which an older window never does: one would end the connection on it.
/// The hold ends with the next push whose seqno is past `seqno`; a push of the pane's title or
/// folder alone during the hold carries `seqno` again. A switch to a held pane holds its first
/// frame for that push, up to a bound (Lachlan's 11a of 2026-11-04)
#[derive(Deserialize, Serialize, PartialEq, Eq, Debug, Clone, Copy)]
pub struct PaneHeld {
    pub pane_id: PaneId,
    pub seqno: SequenceNo,
}

/// LBTerm: window to server, which of the server's tabs are parked (Lachlan's 8a, lbterm/NEW-TAB.md,
/// "Landing 4"; lbterm/MUX.md, "Parked tabs"), answered with [`ParkedTabs`]. Asked after each
/// listing of the panes (an attach, a reconnect, a resync), and the window saying it knows
/// [`TabParked`], which the server pushes only to a connection that has asked this (PITFALLS
/// 188). A server of an older build answers ErrorResponse naming it (`lacks_parked_tabs`): that
/// domain can't park, so its tabs' menus offer no Park tab
#[derive(Deserialize, Serialize, PartialEq, Eq, Debug, Clone, Default)]
pub struct GetParkedTabs {}

/// LBTerm: the answer to [`GetParkedTabs`]: every parked tab of the server's, by its ids
#[derive(Deserialize, Serialize, PartialEq, Eq, Debug, Clone, Default)]
pub struct ParkedTabs {
    pub tabs: Vec<TabId>,
}

/// LBTerm: a tab parked or unparked. Window to server: park or unpark it, answered with
/// UnitResponse, sent only to a server that has answered [`GetParkedTabs`]; the window's own tab
/// isn't set by the window, but by the server's push of it, so nothing it's told goes back
/// (PITFALLS 159). Server to window, unilateral: every change to a tab's flag, its own window's
/// included, sent only on a connection that has asked [`GetParkedTabs`]
#[derive(Deserialize, Serialize, PartialEq, Eq, Debug, Clone, Copy)]
pub struct TabParked {
    pub tab_id: TabId,
    pub parked: bool,
}

/// Whether `error`, the answer to a [`GetParkedTabs`], is a server's that doesn't know the PDU
/// (an older build), rather than any other failure
pub fn lacks_parked_tabs(error: &str) -> bool {
    error.contains(&format!("Invalid {{ ident: {} }}", GET_PARKED_TABS_IDENT))
}

/// [`GetParkedTabs`]'s ident on the wire, as the `pdu!` table numbers it
pub const GET_PARKED_TABS_IDENT: u64 = 7508;

/// LBTerm: client to server, which of a pane's rows `rows` hold a shell's prompt (OSC 133), for
/// the scroll strip's marks (lbterm/SCROLL-STRIP.md, "A shell's marks"): the rows, not their
/// lines, so a client marks a scrollback it doesn't have. A server of an older build answers
/// ErrorResponse, as for any PDU it doesn't know.
#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetPromptRows {
    pub pane_id: PaneId,
    pub rows: Range<StableRowIndex>,
}

/// LBTerm: the answer to GetPromptRows. `looked` is the rows asked for that the pane still has
/// (rows gone from the top of its scrollback aren't), `rows` those of them holding a prompt, and
/// `alt_screen` which of its screens they're on (the alternate one has rows of its own)
#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetPromptRowsResponse {
    pub pane_id: PaneId,
    pub looked: Range<StableRowIndex>,
    pub rows: Vec<StableRowIndex>,
    pub alt_screen: bool,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct TabAddedToWindow {
    pub tab_id: TabId,
    pub window_id: WindowId,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct TabResized {
    pub tab_id: TabId,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct TabTitleChanged {
    pub tab_id: TabId,
    pub title: String,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct WindowTitleChanged {
    pub window_id: WindowId,
    pub title: String,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct PaneFocused {
    pub pane_id: PaneId,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct WindowWorkspaceChanged {
    pub window_id: WindowId,
    pub workspace: String,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SetClientId {
    pub client_id: ClientId,
    pub is_proxy: bool,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SetFocusedPane {
    pub pane_id: PaneId,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetClientList;

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetClientListResponse {
    pub clients: Vec<ClientInfo>,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct Resize {
    pub containing_tab_id: TabId,
    pub pane_id: PaneId,
    pub size: TerminalSize,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SetPaneZoomed {
    pub containing_tab_id: TabId,
    pub pane_id: PaneId,
    pub zoomed: bool,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetPaneDirection {
    pub pane_id: PaneId,
    pub direction: PaneDirection,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct AdjustPaneSize {
    pub pane_id: PaneId,
    pub direction: PaneDirection,
    pub amount: usize,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetPaneDirectionResponse {
    pub pane_id: Option<PaneId>,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct ActivatePaneDirection {
    pub pane_id: PaneId,
    pub direction: PaneDirection,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetPaneRenderChanges {
    pub pane_id: PaneId,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetPaneRenderableDimensions {
    pub pane_id: PaneId,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetPaneRenderableDimensionsResponse {
    pub pane_id: PaneId,
    pub cursor_position: StableCursorPosition,
    pub dimensions: RenderableDimensions,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct LivenessResponse {
    pub pane_id: PaneId,
    pub is_alive: bool,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetPaneRenderChangesResponse {
    pub pane_id: PaneId,
    pub mouse_grabbed: bool,
    pub alt_screen_active: bool,
    pub cursor_position: StableCursorPosition,
    pub dimensions: RenderableDimensions,
    pub dirty_lines: Vec<Range<StableRowIndex>>,
    pub title: String,
    pub working_dir: Option<SerdeUrl>,
    /// Lines that the server thought we'd almost certainly
    /// want to fetch as soon as we received this response
    pub bonus_lines: SerializedLines,

    pub input_serial: Option<InputSerial>,
    pub seqno: SequenceNo,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetLines {
    pub pane_id: PaneId,
    pub lines: Vec<Range<StableRowIndex>>,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
struct CellCoordinates {
    line_idx: usize,
    cols: Range<usize>,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
struct LineHyperlink {
    link: Hyperlink,
    coords: Vec<CellCoordinates>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct SerializedImageCell {
    pub line_idx: StableRowIndex,
    pub cell_idx: usize,
    // The following fields are taken from termwiz::image::ImageCell
    pub top_left: TextureCoordinate,
    pub bottom_right: TextureCoordinate,
    /// Image::data::hash() for the ImageCell::data field
    pub data_hash: [u8; 32],
    pub z_index: i32,
    pub padding_left: u16,
    pub padding_top: u16,
    pub padding_right: u16,
    pub padding_bottom: u16,
    pub image_id: Option<u32>,
    pub placement_id: Option<u32>,
}

/// What's all this?
/// Cells hold references to Arc<Hyperlink> and it is important to us to
/// maintain identity of the hyperlinks in the individual cells, while also
/// only sending a single copy of the associated URL.
/// This section of code extracts the hyperlinks from the cells and builds
/// up a mapping that can be used to restore the identity when the `lines()`
/// method is called.
#[derive(Deserialize, Serialize, PartialEq, Debug, Default)]
pub struct SerializedLines {
    lines: Vec<(StableRowIndex, Line)>,
    hyperlinks: Vec<LineHyperlink>,
    images: Vec<SerializedImageCell>,
}

impl SerializedLines {
    /// LBTerm: whether any of the lines hold an image, which the client fetches by its hash
    /// before the lines can be used (wezterm-client's hydrate_lines), so they can't be taken out
    /// and put back as they are (wezterm-client's lbterm_backlog merges a pane's pushes that way)
    pub fn lbterm_has_images(&self) -> bool {
        !self.images.is_empty()
    }

    /// LBTerm: whether they carry no line at all, as a push of a pane's title and folder alone
    /// doesn't (wezterm-client's lbterm_names applies one on the client thread)
    pub fn lbterm_is_empty(&self) -> bool {
        self.lines.is_empty() && self.images.is_empty()
    }

    /// Reconsitute hyperlinks or other attributes that were decomposed for
    /// serialization, and return the line data.
    pub fn extract_data(self) -> (Vec<(StableRowIndex, Line)>, Vec<SerializedImageCell>) {
        let lines = if self.hyperlinks.is_empty() {
            self.lines
        } else {
            let mut lines = self.lines;

            for link in self.hyperlinks {
                let url = Arc::new(link.link);

                for coord in link.coords {
                    if let Some((_, line)) = lines.get_mut(coord.line_idx) {
                        if let Some(cells) =
                            line.cells_mut_for_attr_changes_only().get_mut(coord.cols)
                        {
                            for cell in cells {
                                cell.attrs_mut().set_hyperlink(Some(Arc::clone(&url)));
                            }
                        }
                    }
                }
            }

            lines
        };
        (lines, self.images)
    }
}

impl From<Vec<(StableRowIndex, Line)>> for SerializedLines {
    fn from(mut lines: Vec<(StableRowIndex, Line)>) -> Self {
        let mut hyperlinks = vec![];
        let mut images = vec![];

        for (line_idx, (stable_row_idx, line)) in lines.iter_mut().enumerate() {
            let mut current_link: Option<Arc<Hyperlink>> = None;
            let mut current_range = 0..0;

            for (x, cell) in line
                .cells_mut_for_attr_changes_only()
                .iter_mut()
                .enumerate()
            {
                // Unset the hyperlink on the cell, if any, and record that
                // in the hyperlinks data for later restoration.
                if let Some(link) = cell.attrs_mut().hyperlink().map(Arc::clone) {
                    cell.attrs_mut().set_hyperlink(None);
                    match current_link.as_ref() {
                        Some(current) if Arc::ptr_eq(&current, &link) => {
                            // Continue the current streak
                            current_range = range_union(current_range, x..x + 1);
                        }
                        Some(prior) => {
                            // It's a different URL, push the current data and start a new one
                            hyperlinks.push(LineHyperlink {
                                link: (**prior).clone(),
                                coords: vec![CellCoordinates {
                                    line_idx,
                                    cols: current_range,
                                }],
                            });
                            current_range = x..x + 1;
                            current_link = Some(link);
                        }
                        None => {
                            // Starting a new streak
                            current_range = x..x + 1;
                            current_link = Some(link);
                        }
                    }
                } else if let Some(link) = current_link.take() {
                    // Wrap up a prior streak
                    hyperlinks.push(LineHyperlink {
                        link: (*link).clone(),
                        coords: vec![CellCoordinates {
                            line_idx,
                            cols: current_range,
                        }],
                    });
                    current_range = 0..0;
                }

                if let Some(cell_images) = cell.attrs().images() {
                    for imcell in cell_images {
                        let (padding_left, padding_top, padding_right, padding_bottom) =
                            imcell.padding();
                        images.push(SerializedImageCell {
                            line_idx: *stable_row_idx,
                            cell_idx: x,
                            top_left: imcell.top_left(),
                            bottom_right: imcell.bottom_right(),
                            z_index: imcell.z_index(),
                            padding_left,
                            padding_top,
                            padding_right,
                            padding_bottom,
                            image_id: imcell.image_id(),
                            placement_id: imcell.placement_id(),
                            data_hash: imcell.image_data().hash(),
                        });
                    }
                }
                cell.attrs_mut().clear_images();
            }
            if let Some(link) = current_link.take() {
                // Wrap up final streak
                hyperlinks.push(LineHyperlink {
                    link: (*link).clone(),
                    coords: vec![CellCoordinates {
                        line_idx,
                        cols: current_range,
                    }],
                });
            }
        }

        Self {
            lines,
            hyperlinks,
            images,
        }
    }
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetLinesResponse {
    pub pane_id: PaneId,
    pub lines: SerializedLines,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct EraseScrollbackRequest {
    pub pane_id: PaneId,
    pub erase_mode: ScrollbackEraseMode,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SearchScrollbackRequest {
    pub pane_id: PaneId,
    pub pattern: mux::pane::Pattern,
    pub range: Range<StableRowIndex>,
    pub limit: Option<u32>,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SearchScrollbackResponse {
    pub results: Vec<mux::pane::SearchResult>,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetImageCell {
    pub pane_id: PaneId,
    pub line_idx: StableRowIndex,
    pub cell_idx: usize,
    pub data_hash: [u8; 32],
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetImageCellResponse {
    pub pane_id: PaneId,
    pub data: Option<Arc<ImageData>>,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_frame() {
        let mut encoded = Vec::new();
        encode_raw(0x81, 0x42, b"hello", false, &mut encoded).unwrap();
        assert_eq!(&encoded, b"\x08\x42\x81\x01hello");
        let decoded = decode_raw(encoded.as_slice()).unwrap();
        assert_eq!(decoded.ident, 0x81);
        assert_eq!(decoded.serial, 0x42);
        assert_eq!(decoded.data, b"hello");
    }

    #[test]
    fn test_frame_lengths() {
        let mut serial = 1;
        for target_len in &[128, 247, 256, 65536, 16777216] {
            let mut payload = Vec::with_capacity(*target_len);
            payload.resize(*target_len, b'a');
            let mut encoded = Vec::new();
            encode_raw(0x42, serial, payload.as_slice(), false, &mut encoded).unwrap();
            let decoded = decode_raw(encoded.as_slice()).unwrap();
            assert_eq!(decoded.ident, 0x42);
            assert_eq!(decoded.serial, serial);
            assert_eq!(decoded.data, payload);
            serial += 1;
        }
    }

    /// LBTerm: what the mux client hands the GUI's counters with each PDU
    #[test]
    fn the_last_pdu_s_payload_size_is_noted() {
        let mut encoded = Vec::new();
        let pdu = Pdu::SetPaneZoomed(SetPaneZoomed {
            containing_tab_id: 1,
            pane_id: 2,
            zoomed: true,
        });
        pdu.encode(&mut encoded, 7).unwrap();
        let sent = lbterm_last_size();
        assert!(sent > 0 && sent < encoded.len(), "{sent} of {}", encoded.len());
        lbterm_note_size(0);
        Pdu::decode(encoded.as_slice()).unwrap();
        assert_eq!(lbterm_last_size(), sent, "the same payload each way");
    }

    #[test]
    fn test_pdu_ping() {
        let mut encoded = Vec::new();
        Pdu::Ping(Ping {}).encode(&mut encoded, 0x40).unwrap();
        assert_eq!(&encoded, &[2, 0x40, 1]);
        assert_eq!(
            DecodedPdu {
                serial: 0x40,
                pdu: Pdu::Ping(Ping {})
            },
            Pdu::decode(encoded.as_slice()).unwrap()
        );
    }

    #[test]
    fn stream_decode() {
        let mut encoded = Vec::new();
        Pdu::Ping(Ping {}).encode(&mut encoded, 0x1).unwrap();
        Pdu::Pong(Pong {}).encode(&mut encoded, 0x2).unwrap();
        assert_eq!(encoded.len(), 6);

        let mut cursor = Cursor::new(encoded.as_slice());
        let mut read_buffer = Vec::new();

        assert_eq!(
            Pdu::try_read_and_decode(&mut cursor, &mut read_buffer).unwrap(),
            Some(DecodedPdu {
                serial: 1,
                pdu: Pdu::Ping(Ping {})
            })
        );
        assert_eq!(
            Pdu::try_read_and_decode(&mut cursor, &mut read_buffer).unwrap(),
            Some(DecodedPdu {
                serial: 2,
                pdu: Pdu::Pong(Pong {})
            })
        );
        let err = Pdu::try_read_and_decode(&mut cursor, &mut read_buffer).unwrap_err();
        assert_eq!(
            err.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn test_pdu_ping_base91() {
        let mut encoded = Vec::new();
        {
            let mut encoder = base91::Base91Encoder::new(&mut encoded);
            Pdu::Ping(Ping {}).encode(&mut encoder, 0x41).unwrap();
        }
        assert_eq!(&encoded, &[60, 67, 75, 65]);
        let decoded = base91::decode(&encoded);
        assert_eq!(
            DecodedPdu {
                serial: 0x41,
                pdu: Pdu::Ping(Ping {})
            },
            Pdu::decode(decoded.as_slice()).unwrap()
        );
    }

    #[test]
    fn test_pdu_pong() {
        let mut encoded = Vec::new();
        Pdu::Pong(Pong {}).encode(&mut encoded, 0x42).unwrap();
        assert_eq!(&encoded, &[2, 0x42, 2]);
        assert_eq!(
            DecodedPdu {
                serial: 0x42,
                pdu: Pdu::Pong(Pong {})
            },
            Pdu::decode(encoded.as_slice()).unwrap()
        );
    }

    #[test]
    fn test_bogus_pdu() {
        let mut encoded = Vec::new();
        encode_raw(0xdeadbeef, 0x42, b"hello", false, &mut encoded).unwrap();
        assert_eq!(
            DecodedPdu {
                serial: 0x42,
                pdu: Pdu::Invalid { ident: 0xdeadbeef }
            },
            Pdu::decode(encoded.as_slice()).unwrap()
        );
    }

    #[test]
    fn lbterm_program_status_round_trip() {
        use std::time::{Duration, UNIX_EPOCH};
        use wezterm_term::program_status::{BlockedKind, ProgramState, ProgramStatusRecord};
        let pdu = || {
            Pdu::SetProgramStatus(SetProgramStatus {
                pane_id: 7,
                records: vec![
                    ProgramStatusRecord {
                        id: String::new(),
                        state: ProgramState::Blocked,
                        kind: Some(BlockedKind::Question),
                        progress: None,
                        app: Some("claude".to_string()),
                        title: Some("Which branch?".to_string()),
                        msg: None,
                        updated: UNIX_EPOCH + Duration::from_millis(1_790_000_000_123),
                    },
                    ProgramStatusRecord {
                        id: "agent/1".to_string(),
                        state: ProgramState::Working,
                        kind: None,
                        progress: Some(40),
                        app: Some("claude".to_string()),
                        title: None,
                        msg: Some("tests".to_string()),
                        updated: UNIX_EPOCH + Duration::from_secs(1_790_000_001),
                    },
                ],
            })
        };
        let mut encoded = Vec::new();
        pdu().encode(&mut encoded, 0x42).unwrap();
        let decoded = Pdu::decode(encoded.as_slice()).unwrap();
        assert_eq!(decoded.serial, 0x42);
        assert_eq!(decoded.pdu, pdu());
        assert_eq!(decoded.pdu.pane_id(), Some(7));
        // Numbered clear of upstream's, so an upstream PDU can't take it
        assert_eq!(decode_raw(encoded.as_slice()).unwrap().ident, 7501);
    }

    #[test]
    fn lbterm_spawn_no_activate_round_trip() {
        let spawn = SpawnV2 {
            domain: config::keyassignment::SpawnTabDomain::DomainName("lbterm".to_string()),
            window_id: Some(3),
            command: Some(CommandBuilder::from_argv(vec!["pwsh".into(), "-NoLogo".into()])),
            command_dir: Some("C:/Users/me".to_string()),
            size: TerminalSize::default(),
            workspace: "default".to_string(),
        };
        let pdu = || {
            Pdu::SpawnNoActivate(SpawnNoActivate {
                spawn: spawn.clone(),
            })
        };
        let mut encoded = Vec::new();
        pdu().encode(&mut encoded, 9).unwrap();
        let decoded = Pdu::decode(encoded.as_slice()).unwrap();
        assert_eq!(decoded.pdu, pdu());
        assert_eq!(decode_raw(encoded.as_slice()).unwrap().ident, 7502);
        // Nobody pressed anything, so the spawn doesn't make its client the
        // one with the latest input
        assert!(!decoded.pdu.is_user_input());
    }

    #[test]
    fn lbterm_prompt_rows_round_trip() {
        let ask = || {
            Pdu::GetPromptRows(GetPromptRows {
                pane_id: 4,
                rows: 1_000..41_000,
            })
        };
        let answer = || {
            Pdu::GetPromptRowsResponse(GetPromptRowsResponse {
                pane_id: 4,
                looked: 1_200..41_000,
                rows: vec![1_499, 1_999, 40_999],
                alt_screen: false,
            })
        };
        for (pdu, ident) in [(ask as fn() -> Pdu, 7503), (answer, 7504)] {
            let mut encoded = Vec::new();
            pdu().encode(&mut encoded, 11).unwrap();
            assert_eq!(Pdu::decode(encoded.as_slice()).unwrap().pdu, pdu());
            assert_eq!(decode_raw(encoded.as_slice()).unwrap().ident, ident);
            // A few bytes, whatever the rows asked about
            assert!(encoded.len() < 64, "{} bytes", encoded.len());
        }
    }

    /// GetParkedTabs, ParkedTabs and TabParked cross as they were sent, under the idents the doc
    /// gives them, and an older server's answer to GetParkedTabs is told from any other failure
    /// (lbterm/MUX.md, "Parked tabs")
    #[test]
    fn lbterm_parked_tabs_round_trip() {
        for (pdu, ident) in [
            (Box::new(|| Pdu::GetParkedTabs(GetParkedTabs {})) as Box<dyn Fn() -> Pdu>, 7508),
            (Box::new(|| Pdu::ParkedTabs(ParkedTabs { tabs: vec![] })), 7509),
            (Box::new(|| Pdu::ParkedTabs(ParkedTabs { tabs: vec![3, 41] })), 7509),
            (Box::new(|| Pdu::TabParked(TabParked { tab_id: 7, parked: true })), 7510),
            (Box::new(|| Pdu::TabParked(TabParked { tab_id: 7, parked: false })), 7510),
        ] {
            let mut encoded = Vec::new();
            pdu().encode(&mut encoded, 9).unwrap();
            let decoded = Pdu::decode(encoded.as_slice()).unwrap();
            assert_eq!(decoded, DecodedPdu { serial: 9, pdu: pdu() });
            assert_eq!(decode_raw(encoded.as_slice()).unwrap().ident, ident);
            // TabParked names no pane: a window's client thread hands it on by its kind
            // (wezterm-client's process_unilateral), as it does TabTitleChanged
            assert_eq!(pdu().pane_id(), None);
        }
        assert_eq!(GET_PARKED_TABS_IDENT, 7508, "the ident lacks_parked_tabs looks for");
        let answer = |pdu: Pdu| format!("Error: {:#}", anyhow::anyhow!("invalid PDU {:?}", pdu));
        assert!(lacks_parked_tabs(&answer(Pdu::Invalid { ident: 7508 })));
        assert!(!lacks_parked_tabs(&answer(Pdu::Invalid { ident: 7505 })));
        assert!(!lacks_parked_tabs("Error: no such tab 4"));
    }

    /// A window's set of panes shown, as many as a few windows' active tabs hold
    fn lbterm_shown(panes: std::ops::Range<PaneId>, drawn: Option<SequenceNo>) -> Pdu {
        Pdu::SetShownPanes(SetShownPanes {
            panes: panes.map(|pane_id| ShownPane { pane_id, drawn }).collect(),
        })
    }

    /// SetShownPanes and PaneHeld cross as they were sent, under the idents the doc gives
    /// them, and say what they say in a few bytes: one goes at each tab switch and one a pane's
    /// hold (lbterm/MUX.md, "Tabs out of view: output held at the server")
    #[test]
    fn lbterm_shown_panes_and_held_round_trip() {
        let held = || Pdu::PaneHeld(PaneHeld { pane_id: 12, seqno: 81_234 });
        for (pdu, ident) in [
            (Box::new(|| lbterm_shown(3..4, None)) as Box<dyn Fn() -> Pdu>, 7505),
            (Box::new(|| lbterm_shown(3..7, Some(2_000_000))), 7505),
            (Box::new(|| lbterm_shown(0..0, None)), 7505),
            (Box::new(held), 7506),
        ] {
            let mut encoded = Vec::new();
            pdu().encode(&mut encoded, 21).unwrap();
            let decoded = Pdu::decode(encoded.as_slice()).unwrap();
            assert_eq!(decoded, DecodedPdu { serial: 21, pdu: pdu() });
            assert_eq!(decode_raw(encoded.as_slice()).unwrap().ident, ident);
            assert!(encoded.len() < 48, "{} bytes for {:?}", encoded.len(), pdu());
        }
        assert_eq!(SET_SHOWN_PANES_IDENT, 7505, "the ident lacks_shown_panes looks for");
        // PaneHeld names its pane, so the window's client thread hands it on with the pane's
        // other PDUs (wezterm-client's process_unilateral) rather than ending the connection
        assert_eq!(held().pane_id(), Some(12));
        assert_eq!(lbterm_shown(3..5, None).pane_id(), None);
    }

    /// A frame's word that it read a pane's push, as a window says it each paint
    fn lbterm_drawn(panes: &[(PaneId, SequenceNo)]) -> Pdu {
        Pdu::PanesDrawn(PanesDrawn {
            panes: panes
                .iter()
                .map(|&(pane_id, seqno)| PaneDrawn { pane_id, seqno })
                .collect(),
        })
    }

    /// PanesDrawn crosses as it was sent, at serial 0 (a notice: no request waits on it), under
    /// the ident the doc gives it, in a few bytes: one goes a paint that read a new push of a
    /// shown pane, up to the frame rate (lbterm/MUX.md, "A shown pane's pace"). It names no one
    /// pane, so a window that got one (no server sends it) would end the connection on it, as
    /// on any PDU it can't hand on
    #[test]
    fn lbterm_panes_drawn_round_trip() {
        for panes in [
            vec![(3, 81_234)],
            vec![(3, 1), (4, 2_000_000), (17, SequenceNo::MAX)],
            vec![],
        ] {
            let mut encoded = Vec::new();
            lbterm_drawn(&panes).encode(&mut encoded, 0).unwrap();
            let decoded = Pdu::decode(encoded.as_slice()).unwrap();
            assert_eq!(decoded, DecodedPdu { serial: 0, pdu: lbterm_drawn(&panes) });
            let raw = decode_raw(encoded.as_slice()).unwrap();
            assert_eq!((raw.ident, raw.serial), (7507, 0));
            assert!(encoded.len() < 48, "{} bytes for {:?}", encoded.len(), panes);
        }
        assert_eq!(lbterm_drawn(&[(3, 9)]).pane_id(), None);
        assert!(!lbterm_drawn(&[(3, 9)]).is_user_input(), "nobody pressed anything");
    }

    /// How a server that doesn't know PanesDrawn takes it: as SetShownPanes, `Pdu::Invalid` at
    /// its serial, here 0, the frame after it read whole; its sessionhandler answers the Invalid
    /// with ErrorResponse at serial 0, which a window's client thread would take as a push and
    /// end the connection on (lbterm/PITFALLS.md, 227). So a window sends it only to a server that
    /// has answered SetShownPanes, and this stands in for the older server under an ident this
    /// build doesn't know
    #[test]
    fn lbterm_panes_drawn_to_an_older_server() {
        let mut ours = Vec::new();
        lbterm_drawn(&[(1, 40), (2, 41)]).encode(&mut ours, 0).unwrap();
        let frame = decode_raw(ours.as_slice()).unwrap();
        let mut bytes = Vec::new();
        encode_raw(7598, frame.serial, &frame.data, frame.is_compressed, &mut bytes).unwrap();
        Pdu::Ping(Ping {}).encode(&mut bytes, 5).unwrap();

        let mut socket = LbtermSocket { data: bytes, reads: 0 };
        let mut reader = PduReader::new();
        let got = smol::block_on(reader.decode_async(&mut socket, None)).unwrap();
        assert_eq!(got, DecodedPdu { serial: 0, pdu: Pdu::Invalid { ident: 7598 } });
        let got = smol::block_on(reader.decode_async(&mut socket, None)).unwrap();
        assert_eq!(got, DecodedPdu { serial: 5, pdu: Pdu::Ping(Ping {}) });
    }

    /// How an older peer takes them. A server of a build before SetShownPanes reads its frame
    /// as Pdu::Invalid, keeping its serial, and the frame after it whole, so the connection goes
    /// on; its sessionhandler answers the Invalid with ErrorResponse, which the window knows by
    /// `lacks_shown_panes`. Stood in for here by the same frame under an ident this build doesn't
    /// know, both plain and over zstd's threshold (a set of many panes, or seqnos said)
    #[test]
    fn lbterm_shown_panes_to_an_older_server() {
        for shown in [lbterm_shown(1..3, None), lbterm_shown(0..40, Some(7_000))] {
            let mut ours = Vec::new();
            shown.encode(&mut ours, 33).unwrap();
            let frame = decode_raw(ours.as_slice()).unwrap();
            let mut bytes = Vec::new();
            encode_raw(7599, frame.serial, &frame.data, frame.is_compressed, &mut bytes).unwrap();
            Pdu::Ping(Ping {}).encode(&mut bytes, 34).unwrap();

            // As upstream's decode reads it (a cli, a TLS or ssh peer)
            let mut cursor = std::io::Cursor::new(bytes.as_slice());
            assert_eq!(
                Pdu::decode(&mut cursor).unwrap(),
                DecodedPdu { serial: 33, pdu: Pdu::Invalid { ident: 7599 } }
            );
            assert_eq!(
                Pdu::decode(&mut cursor).unwrap(),
                DecodedPdu { serial: 34, pdu: Pdu::Ping(Ping {}) }
            );

            // As the mux server reads its client's PDUs (PduReader)
            let mut socket = LbtermSocket { data: bytes, reads: 0 };
            let mut reader = PduReader::new();
            let got = smol::block_on(reader.decode_async(&mut socket, None)).unwrap();
            assert_eq!(got, DecodedPdu { serial: 33, pdu: Pdu::Invalid { ident: 7599 } });
            let got = smol::block_on(reader.decode_async(&mut socket, None)).unwrap();
            assert_eq!(got, DecodedPdu { serial: 34, pdu: Pdu::Ping(Ping {}) });
        }

        // The older server's answer, as its sessionhandler words it (`send_response` of
        // `anyhow!("invalid PDU {:?}", pdu)`), and nothing else that can go wrong
        let answer = |pdu: Pdu| format!("Error: {:#}", anyhow::anyhow!("invalid PDU {:?}", pdu));
        assert!(lacks_shown_panes(&answer(Pdu::Invalid { ident: 7505 })));
        assert!(!lacks_shown_panes(&answer(Pdu::Invalid { ident: 75050 })));
        assert!(!lacks_shown_panes(&answer(Pdu::Invalid { ident: 7503 })));
        assert!(!lacks_shown_panes("Error: pane 4 not found"));
    }

    /// Scrollback as the GUI fetches it with GetLines
    fn lbterm_scrollback(rows: usize) -> Pdu {
        use termwiz::cell::CellAttributes;
        let lines: Vec<(StableRowIndex, Line)> = (0..rows)
            .map(|row| {
                let text = format!(
                    "{row:>6}    Compiling wezterm-gui v0.1.0 (C:\\Users\\me\\Code\\LBTerm\\wezterm-gui)"
                );
                let line = Line::from_text(
                    &text,
                    &CellAttributes::default(),
                    termwiz::surface::SEQ_ZERO,
                    None,
                );
                (row as StableRowIndex, line)
            })
            .collect();
        Pdu::GetLinesResponse(GetLinesResponse {
            pane_id: 7,
            lines: lines.into(),
        })
    }

    #[test]
    fn lbterm_pdu_that_fails_to_compress_goes_uncompressed() {
        // 2,000 rows of a build's output
        let rows = 2_000;
        let Pdu::GetLinesResponse(response) = lbterm_scrollback(rows) else {
            unreachable!()
        };

        // As it goes when zstd has its memory: compressed, and back whole
        let mut normal = Vec::new();
        lbterm_scrollback(rows).encode(&mut normal, 5).unwrap();
        let frame = decode_raw(normal.as_slice()).unwrap();
        assert!(frame.is_compressed);
        assert_eq!(
            Pdu::decode(normal.as_slice()).unwrap().pdu,
            lbterm_scrollback(rows)
        );

        // zstd failing to allocate its workspace, as in the mux server on
        // 2026-10-21: that used to fail the encode, and the server dropped
        // the client for it. Now the frame goes uncompressed and decodes to
        // the same PDU.
        let (data, is_compressed) = serialize_with(&response, |_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Allocation error : not enough memory",
            )
            .into())
        })
        .unwrap();
        assert!(!is_compressed);
        assert!(data.len() > frame.data.len());
        let mut encoded = Vec::new();
        encode_raw(frame.ident, 5, &data, is_compressed, &mut encoded).unwrap();
        let decoded = Pdu::decode(encoded.as_slice()).unwrap();
        assert_eq!(decoded.serial, 5);
        assert_eq!(decoded.pdu, lbterm_scrollback(rows));
    }

    // LBTerm: upstream's way of compressing and decompressing PDUs, as it was
    // before the zstd contexts were kept (`KeptZstd`), so the tests can hold
    // the bytes on the wire to it, both ways

    /// Upstream's serialize: the PDU serialized and, over COMPRESS_THRESH,
    /// serialized again into a new zstd stream encoder
    fn upstream_serialize<T: serde::Serialize>(t: &T) -> (Vec<u8>, bool) {
        let mut uncompressed = Vec::new();
        let mut encode = varbincode::Serializer::new(&mut uncompressed);
        t.serialize(&mut encode).unwrap();
        if uncompressed.len() <= COMPRESS_THRESH {
            return (uncompressed, false);
        }
        let mut compressed = Vec::new();
        let mut compress =
            zstd::Encoder::new(&mut compressed, zstd::DEFAULT_COMPRESSION_LEVEL).unwrap();
        let mut encode = varbincode::Serializer::new(&mut compress);
        t.serialize(&mut encode).unwrap();
        drop(encode);
        compress.finish().unwrap();
        if compressed.len() < uncompressed.len() {
            (compressed, true)
        } else {
            (uncompressed, false)
        }
    }

    /// Upstream's deserialize: a new zstd stream decoder for each compressed PDU
    fn upstream_deserialize<T: serde::de::DeserializeOwned>(mut r: &[u8], is_compressed: bool) -> T {
        if is_compressed {
            let mut decompress = zstd::Decoder::new(r).unwrap();
            let mut decode = varbincode::Deserializer::new(&mut decompress);
            serde::Deserialize::deserialize(&mut decode).unwrap()
        } else {
            let mut decode = varbincode::Deserializer::new(&mut r);
            serde::Deserialize::deserialize(&mut decode).unwrap()
        }
    }

    /// A line of coloured words, as a build's output or Claude Code's is
    fn lbterm_coloured_line(row: usize, cols: usize) -> Line {
        use termwiz::cell::CellAttributes;
        use termwiz::color::AnsiColor;
        let words = [
            "Compiling",
            "wezterm-gui",
            "v0.1.0",
            "warning:",
            "unused",
            "variable",
            "src/lib.rs:120",
            "Finished",
            "\u{25cf}",
            "\u{276f}",
        ];
        let colours = [
            AnsiColor::Green,
            AnsiColor::Yellow,
            AnsiColor::Silver,
            AnsiColor::Aqua,
            AnsiColor::Fuchsia,
        ];
        let mut line = Line::from_text(
            "",
            &CellAttributes::default(),
            termwiz::surface::SEQ_ZERO,
            None,
        );
        let (mut used, mut i) = (0, row);
        while used < cols {
            let word = format!("{} ", words[i % words.len()]);
            let mut attrs = CellAttributes::default();
            attrs.set_foreground(colours[i % colours.len()]);
            used += word.chars().count();
            line.append_line(
                Line::from_text(&word, &attrs, termwiz::surface::SEQ_ZERO, None),
                termwiz::surface::SEQ_ZERO,
            );
            i += 3;
        }
        line
    }

    /// What the server pushes a window when a pane's output changes `rows` lines
    fn lbterm_push(rows: usize, seqno: SequenceNo) -> GetPaneRenderChangesResponse {
        use std::convert::TryFrom;
        let top: StableRowIndex = 1000;
        let lines: Vec<(StableRowIndex, Line)> = (0..rows)
            .map(|r| {
                (
                    top + r as StableRowIndex,
                    lbterm_coloured_line(r + seqno, 120),
                )
            })
            .collect();
        GetPaneRenderChangesResponse {
            pane_id: 7,
            mouse_grabbed: false,
            alt_screen_active: true,
            cursor_position: StableCursorPosition {
                x: 4,
                y: top + rows as StableRowIndex,
                ..Default::default()
            },
            dimensions: RenderableDimensions {
                cols: 120,
                viewport_rows: 40,
                scrollback_rows: 3040,
                physical_top: top,
                scrollback_top: 0,
                dpi: 96,
                pixel_width: 1200,
                pixel_height: 800,
                reverse_video: false,
            },
            dirty_lines: vec![top..top + rows as StableRowIndex],
            title: "\u{2733} Claude Code".to_string(),
            working_dir: Some(SerdeUrl::try_from("file:///C:/work/demo".to_string()).unwrap()),
            bonus_lines: lines.into(),
            input_serial: None,
            seqno,
        }
    }

    /// Bytes no compressor can shrink
    fn lbterm_noise(len: usize, seed: u64) -> Vec<u8> {
        let mut x = seed | 1;
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 24) as u8
            })
            .collect()
    }

    /// The payload LBTerm makes of `t` is upstream's to the byte, compressed
    /// or not as upstream's was, and each path decodes the other's
    fn lbterm_same_as_upstream<T>(t: &T) -> (usize, bool)
    where
        T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let (data, is_compressed) = serialize(t).unwrap();
        let (up, up_compressed) = upstream_serialize(t);
        assert_eq!(is_compressed, up_compressed, "compressed or not, as upstream");
        assert!(
            data == up,
            "{} bytes (compressed {is_compressed}) against upstream's {}",
            data.len(),
            up.len()
        );
        assert_eq!(&deserialize::<T>(&up, up_compressed).unwrap(), t);
        assert_eq!(&upstream_deserialize::<T>(&data, is_compressed), t);
        (data.len(), is_compressed)
    }

    /// The bytes on the wire are upstream's, PDU by PDU, from contexts kept
    /// across PDUs of every size in turn, as the server's thread sends them:
    /// pushes of a few lines to a full screen, a key, status records, just
    /// over the threshold, scrollback over zstd's 128 KB block, output over
    /// its 2 MB window, and noise that goes uncompressed
    #[test]
    fn lbterm_pdus_go_on_the_wire_as_upstream_s_did() {
        use std::time::{Duration, UNIX_EPOCH};
        use termwiz::input::{KeyCode, KeyEvent, Modifiers};
        use wezterm_term::program_status::{ProgramState, ProgramStatusRecord};

        let key = SendKeyDown {
            pane_id: 7,
            event: KeyEvent {
                key: KeyCode::Char('x'),
                modifiers: Modifiers::NONE,
            },
            input_serial: InputSerial::from(UNIX_EPOCH + Duration::from_millis(1_790_000_000_123)),
        };
        let status = SetProgramStatus {
            pane_id: 7,
            records: vec![ProgramStatusRecord {
                id: String::new(),
                state: ProgramState::Working,
                kind: None,
                progress: Some(40),
                app: Some("claude".to_string()),
                title: Some("Reading codec/src/lib.rs".to_string()),
                msg: None,
                updated: UNIX_EPOCH + Duration::from_millis(1_790_000_000_456),
            }],
        };
        // 33 bytes serialized: compression is tried, and loses
        let just_over = ErrorResponse {
            reason: "x".repeat(COMPRESS_THRESH),
        };
        let Pdu::GetLinesResponse(scrollback) = lbterm_scrollback(2_000) else {
            unreachable!()
        };
        let output = WriteToPane {
            pane_id: 7,
            data: (0..60_000)
                .flat_map(|n| format!("{n:>8} building crate {} of 870\r\n", n % 870).into_bytes())
                .collect(),
        };
        assert!(output.data.len() > 2 << 20, "over zstd's window");
        let noise = WriteToPane {
            pane_id: 7,
            data: lbterm_noise(300_000, 7),
        };

        let mut compressed = 0;
        let mut plain = 0;
        let mut count = |(_, is_compressed): (usize, bool)| {
            if is_compressed {
                compressed += 1
            } else {
                plain += 1
            }
        };
        for round in 0..3 {
            for rows in [1, 3, 0, 40, 2, 12] {
                count(lbterm_same_as_upstream(&lbterm_push(rows, round * 10 + rows)));
            }
            count(lbterm_same_as_upstream(&key));
            count(lbterm_same_as_upstream(&status));
            count(lbterm_same_as_upstream(&just_over));
            match round {
                0 => count(lbterm_same_as_upstream(&scrollback)),
                1 => count(lbterm_same_as_upstream(&output)),
                _ => count(lbterm_same_as_upstream(&noise)),
            }
            count(lbterm_same_as_upstream(&lbterm_push(3, 99)));
        }
        assert!(compressed >= 20, "{} compressed", compressed);
        assert!(plain >= 3, "{} sent as they were", plain);

        // And the frame around it: a push as Pdu::encode writes it is
        // upstream's frame of upstream's payload
        let mut encoded = Vec::new();
        Pdu::GetPaneRenderChangesResponse(lbterm_push(3, 5))
            .encode(&mut encoded, 0x42)
            .unwrap();
        let (data, is_compressed) = upstream_serialize(&lbterm_push(3, 5));
        let mut upstream = Vec::new();
        encode_raw(25, 0x42, &data, is_compressed, &mut upstream).unwrap();
        assert_eq!(encoded, upstream);
        assert_eq!(
            Pdu::decode(upstream.as_slice()).unwrap().pdu,
            Pdu::GetPaneRenderChangesResponse(lbterm_push(3, 5))
        );
    }

    /// A thread makes one encoder and one decoder, however many PDUs it
    /// compresses and decompresses (upstream made one of each per PDU)
    #[test]
    fn lbterm_a_thread_keeps_one_zstd_encoder_and_decoder() {
        let made = std::thread::spawn(|| {
            for n in 0..200 {
                let pdu = Pdu::GetPaneRenderChangesResponse(lbterm_push(1 + n % 5, n));
                let mut encoded = Vec::new();
                pdu.encode(&mut encoded, n as u64 + 1).unwrap();
                assert!(decode_raw(encoded.as_slice()).unwrap().is_compressed);
                assert_eq!(Pdu::decode(encoded.as_slice()).unwrap().pdu, pdu);
            }
            KEPT_ZSTD.with(|kept| {
                let kept = kept.borrow();
                (kept.encoders_made, kept.decoders_made)
            })
        })
        .join()
        .unwrap();
        assert_eq!(made, (1, 1), "encoders and decoders made for 200 PDUs");
    }

    /// A decompressed PDU's bytes are kept for the next only while they're small
    #[test]
    fn lbterm_a_big_pdu_s_bytes_go_once_it_s_decoded() {
        let mut encoded = Vec::new();
        lbterm_scrollback(2_000).encode(&mut encoded, 3).unwrap();
        assert!(decode_raw(encoded.as_slice()).unwrap().is_compressed);
        assert_eq!(
            Pdu::decode(encoded.as_slice()).unwrap().pdu,
            lbterm_scrollback(2_000)
        );
        let kept = KEPT_ZSTD.with(|kept| kept.borrow().decompressed.capacity());
        assert_eq!(kept, 0, "a scrollback's bytes kept");
        let mut encoded = Vec::new();
        Pdu::GetPaneRenderChangesResponse(lbterm_push(3, 1))
            .encode(&mut encoded, 4)
            .unwrap();
        Pdu::decode(encoded.as_slice()).unwrap();
        let kept = KEPT_ZSTD.with(|kept| kept.borrow().decompressed.capacity());
        assert!(kept > 0 && kept <= KEEP_DECOMPRESSED, "{}", kept);
    }

    /// A frame cut short in its zstd data fails to decode, and the next
    /// decodes as it should
    #[test]
    fn lbterm_a_cut_frame_fails_and_the_next_decodes() {
        let (data, is_compressed) = serialize(&lbterm_push(12, 2)).unwrap();
        assert!(is_compressed);
        let cut = &data[..data.len() / 2];
        assert!(deserialize::<GetPaneRenderChangesResponse>(cut, true).is_err());
        assert_eq!(
            deserialize::<GetPaneRenderChangesResponse>(&data, true).unwrap(),
            lbterm_push(12, 2)
        );
    }

    /// An AsyncRead over bytes that hands them out `sizes` at a time in turn
    /// (all it's asked for when there are none), counting its reads
    #[derive(Debug)]
    struct LbtermChunks {
        data: Vec<u8>,
        pos: usize,
        sizes: Vec<usize>,
        reads: usize,
    }

    impl LbtermChunks {
        fn new(data: Vec<u8>, sizes: &[usize]) -> Self {
            Self {
                data,
                pos: 0,
                sizes: sizes.to_vec(),
                reads: 0,
            }
        }
    }

    impl smol::io::AsyncRead for LbtermChunks {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &mut [u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            let me = &mut *self;
            let size = if me.sizes.is_empty() {
                usize::MAX
            } else {
                me.sizes[me.reads % me.sizes.len()]
            };
            me.reads += 1;
            let n = size.min(buf.len()).min(me.data.len() - me.pos);
            buf[..n].copy_from_slice(&me.data[me.pos..me.pos + n]);
            me.pos += n;
            std::task::Poll::Ready(Ok(n))
        }
    }

    /// Frames of every kind and size: pings, pushes, a key, a frame bigger
    /// than a PduReader's read (uncompressed noise), scrollback
    fn lbterm_frames() -> (Vec<u8>, Vec<DecodedPdu>) {
        let mut bytes = Vec::new();
        let mut pdus = Vec::new();
        for n in 0..60u64 {
            let pdu = match n % 6 {
                0 => Pdu::Ping(Ping {}),
                1 | 2 => Pdu::GetPaneRenderChangesResponse(lbterm_push(
                    1 + n as usize % 4,
                    n as usize,
                )),
                3 => Pdu::WriteToPane(WriteToPane {
                    pane_id: 1,
                    data: if n == 21 {
                        lbterm_noise(PDU_READ_CHUNK * 3, n)
                    } else {
                        b"hello".to_vec()
                    },
                }),
                4 => Pdu::Pong(Pong {}),
                _ => {
                    if n == 41 {
                        lbterm_scrollback(300)
                    } else {
                        Pdu::UnitResponse(UnitResponse {})
                    }
                }
            };
            pdu.encode(&mut bytes, n + 1).unwrap();
            pdus.push(DecodedPdu { serial: n + 1, pdu });
        }
        (bytes, pdus)
    }

    /// Every frame comes off a PduReader whole and in order, however the reads
    /// cut them (a byte at a time, mid-header, mid-data, many at once), and
    /// then the end of the stream is UnexpectedEof, as from decode_async
    #[test]
    fn lbterm_pdu_reader_takes_frames_off_whatever_reads_bring() {
        let (bytes, want) = lbterm_frames();
        for sizes in [vec![1], vec![2, 1, 3], vec![7, 4096, 13, 1, 65536], vec![]] {
            let mut stream = LbtermChunks::new(bytes.clone(), &sizes);
            let mut reader = PduReader::new();
            smol::block_on(async {
                for want in &want {
                    let got = reader.decode_async(&mut stream, Some(1000)).await.unwrap();
                    assert_eq!(&got, want, "reads of {sizes:?}");
                }
                assert!(!reader.has_buffered());
                let err = reader.decode_async(&mut stream, None).await.unwrap_err();
                let io = err.root_cause().downcast_ref::<std::io::Error>().unwrap();
                assert_eq!(io.kind(), std::io::ErrorKind::UnexpectedEof);
            });
        }
    }

    /// A socket with `data` arrived so far: a read with nothing to give would block
    struct LbtermSocket {
        data: Vec<u8>,
        reads: usize,
    }

    impl smol::io::AsyncRead for LbtermSocket {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &mut [u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            let me = &mut *self;
            me.reads += 1;
            if me.data.is_empty() {
                return std::task::Poll::Pending;
            }
            let n = buf.len().min(me.data.len());
            buf[..n].copy_from_slice(&me.data[..n]);
            me.data.drain(..n);
            std::task::Poll::Ready(Ok(n))
        }
    }

    /// read_now says whether a PDU's bytes wait without waiting for them, and
    /// keeps what it read for the decode after
    #[test]
    fn lbterm_pdu_reader_reads_now_without_waiting() {
        let mut socket = LbtermSocket {
            data: vec![],
            reads: 0,
        };
        let mut reader = PduReader::new();
        assert!(!reader.read_now(&mut socket), "nothing there: no wait");
        assert_eq!(socket.reads, 1);
        let pdu = Pdu::GetPaneRenderChangesResponse(lbterm_push(2, 5));
        pdu.encode(&mut socket.data, 9).unwrap();
        assert!(reader.read_now(&mut socket));
        assert!(socket.data.is_empty(), "read into the reader");
        assert!(reader.read_now(&mut socket), "what's buffered says so without a read");
        assert_eq!(socket.reads, 2);
        let got = smol::block_on(reader.decode_async(&mut socket, None)).unwrap();
        assert_eq!(got, DecodedPdu { serial: 9, pdu });
        assert!(!reader.read_now(&mut socket));
    }

    /// A PDU off a PduReader notes its payload's size, as decode_async does,
    /// for the GUI's counters (the mux client reads every PDU this way)
    #[test]
    fn lbterm_pdu_reader_notes_the_payload_size() {
        let mut bytes = Vec::new();
        let pdu = Pdu::GetPaneRenderChangesResponse(lbterm_push(1, 3));
        pdu.encode(&mut bytes, 1).unwrap();
        let sent = lbterm_last_size();
        assert!(sent > 0);
        lbterm_note_size(0);
        smol::block_on(async {
            let mut stream = LbtermChunks::new(bytes.clone(), &[]);
            PduReader::new()
                .decode_async(&mut stream, None)
                .await
                .unwrap();
        });
        assert_eq!(lbterm_last_size(), sent, "the same payload each way");
    }

    /// When frames come together, one read brings many: upstream's took
    /// four to eight for each
    #[test]
    fn lbterm_pdu_reader_reads_once_for_many_frames() {
        let mut bytes = Vec::new();
        for n in 0..100 {
            Pdu::GetPaneRenderChangesResponse(lbterm_push(1, n))
                .encode(&mut bytes, n as u64 + 1)
                .unwrap();
        }
        assert!(bytes.len() < PDU_READ_CHUNK, "{} bytes", bytes.len());
        let (ours, upstream) = smol::block_on(async {
            let mut stream = LbtermChunks::new(bytes.clone(), &[]);
            let mut reader = PduReader::new();
            for _ in 0..100 {
                reader.decode_async(&mut stream, None).await.unwrap();
                assert!(stream.reads <= 1, "{} reads", stream.reads);
            }
            let ours = stream.reads;
            let mut stream = LbtermChunks::new(bytes.clone(), &[]);
            for _ in 0..100 {
                Pdu::decode_async(&mut stream, None).await.unwrap();
            }
            (ours, stream.reads)
        });
        assert_eq!(ours, 1);
        assert!(upstream >= 400, "upstream's {} reads", upstream);
    }

    /// A frame cut off by the end of the stream is UnexpectedEof, and a serial
    /// past max_serial is a CorruptResponse, as from decode_async
    #[test]
    fn lbterm_pdu_reader_fails_as_decode_async_does() {
        let mut bytes = Vec::new();
        Pdu::GetPaneRenderChangesResponse(lbterm_push(3, 1))
            .encode(&mut bytes, 5)
            .unwrap();
        smol::block_on(async {
            for cut in [1, 2, 3, bytes.len() - 1] {
                let mut stream = LbtermChunks::new(bytes[..cut].to_vec(), &[]);
                let err = PduReader::new()
                    .decode_async(&mut stream, None)
                    .await
                    .unwrap_err();
                let io = err.root_cause().downcast_ref::<std::io::Error>();
                assert_eq!(
                    io.map(|e| e.kind()),
                    Some(std::io::ErrorKind::UnexpectedEof),
                    "cut at {cut}: {err:#}"
                );
                let mut stream = LbtermChunks::new(bytes[..cut].to_vec(), &[]);
                let upstream = Pdu::decode_async(&mut stream, None).await.unwrap_err();
                let io = upstream.root_cause().downcast_ref::<std::io::Error>();
                assert_eq!(
                    io.map(|e| e.kind()),
                    Some(std::io::ErrorKind::UnexpectedEof),
                    "upstream, cut at {cut}"
                );
            }
            let mut stream = LbtermChunks::new(bytes.clone(), &[]);
            let err = PduReader::new()
                .decode_async(&mut stream, Some(4))
                .await
                .unwrap_err();
            assert!(err.root_cause().is::<CorruptResponse>(), "{:#}", err);
            let mut stream = LbtermChunks::new(bytes.clone(), &[]);
            let err = Pdu::decode_async(&mut stream, Some(4)).await.unwrap_err();
            assert!(err.root_cause().is::<CorruptResponse>(), "upstream: {:#}", err);
            // A frame whose length is too short for its own header
            let mut stream = LbtermChunks::new(vec![1, 0x42, 0x81, 0x01], &[]);
            let err = PduReader::new()
                .decode_async(&mut stream, None)
                .await
                .unwrap_err();
            assert!(err.root_cause().is::<CorruptResponse>(), "{:#}", err);
            // A length whose leb128 never ends fails once it fills the reader,
            // rather than reading on (upstream's read until the stream ended)
            for sizes in [vec![], vec![1000]] {
                let mut stream = LbtermChunks::new(vec![0xff; PDU_READ_CHUNK * 2], &sizes);
                let err = PduReader::new()
                    .decode_async(&mut stream, None)
                    .await
                    .unwrap_err();
                let io = err.root_cause().downcast_ref::<std::io::Error>();
                assert_eq!(
                    io.map(|e| e.kind()),
                    Some(std::io::ErrorKind::InvalidData),
                    "{:#}",
                    err
                );
                assert_eq!(stream.pos, PDU_READ_CHUNK, "read no further");
            }
        });
    }

    #[test]
    fn lbterm_frame_header_parses_as_decode_raw_reads_it() {
        let mut encoded = Vec::new();
        encode_raw(0x81, 0x42, b"hello", true, &mut encoded).unwrap();
        assert_eq!(
            parse_frame_header(&encoded, None).unwrap(),
            Some(FrameHeader {
                header_len: encoded.len() - 5,
                ident: 0x81,
                serial: 0x42,
                data_len: 5,
                is_compressed: true,
            })
        );
        for cut in 0..encoded.len() - 5 {
            assert_eq!(
                parse_frame_header(&encoded[..cut], None).unwrap(),
                None,
                "cut at {cut}"
            );
        }
        // A leb128 longer than a u64 is read to its end, as upstream's
        // byte-at-a-time read did, and then refused
        let mut too_long = vec![0xff; 12];
        assert_eq!(parse_frame_header(&too_long, None).unwrap(), None);
        too_long.push(0x7f);
        assert!(
            parse_frame_header(&too_long, None).is_err(),
            "a leb128 too long"
        );
    }
}
