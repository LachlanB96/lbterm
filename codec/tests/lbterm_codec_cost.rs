//! LBTerm: what encoding and decoding a PDU costs, upstream's way against
//! LBTerm's, measured in a process of its own (a test binary of one test) so
//! its page faults are its own. Upstream's way is rebuilt here from codec's
//! own dependencies as `codec/src/lib.rs` had it before LBTerm kept its zstd
//! contexts and buffered its reads.
//!
//! It fails when encoding or decoding a push takes page faults again, as a
//! zstd context made for each PDU did (its workspace mapped, and its tables
//! cleared, each time), or when frames that arrived together take more than
//! one read. The timings go to `<target>/tmp/lbterm-codec-cost.json`, and
//! `LBTERM_CODEC_BENCH=<n>` runs n times as many PDUs.

use codec::*;
use mux::renderable::{RenderableDimensions, StableCursorPosition};
use mux::tab::SerdeUrl;
use serde::Serialize;
use std::convert::TryFrom;
use std::time::Instant;
use termwiz::surface::{Line, SequenceNo, SEQ_ZERO};
use wezterm_term::StableRowIndex;

const COMPRESS_THRESH: usize = 32;
const COMPRESSED_MASK: u64 = 1 << 63;

fn coloured_line(row: usize, cols: usize) -> Line {
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
    let mut line = Line::from_text("", &CellAttributes::default(), SEQ_ZERO, None);
    let (mut used, mut i) = (0, row);
    while used < cols {
        let word = format!("{} ", words[i % words.len()]);
        let mut attrs = CellAttributes::default();
        attrs.set_foreground(colours[i % colours.len()]);
        used += word.chars().count();
        line.append_line(Line::from_text(&word, &attrs, SEQ_ZERO, None), SEQ_ZERO);
        i += 3;
    }
    line
}

/// What the server pushes a window when a pane's output changes `rows` lines
fn push(rows: usize, seqno: SequenceNo) -> GetPaneRenderChangesResponse {
    let top: StableRowIndex = 1000;
    let lines: Vec<(StableRowIndex, Line)> = (0..rows)
        .map(|r| (top + r as StableRowIndex, coloured_line(r + seqno, 120)))
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

/// Upstream's frame of `t`: serialized, and over the threshold serialized
/// again into a new zstd stream encoder
fn upstream_frame<T: Serialize>(ident: u64, serial: u64, t: &T) -> Vec<u8> {
    let mut uncompressed = Vec::new();
    t.serialize(&mut varbincode::Serializer::new(&mut uncompressed))
        .unwrap();
    let (data, is_compressed) = if uncompressed.len() <= COMPRESS_THRESH {
        (uncompressed, false)
    } else {
        let mut compressed = Vec::new();
        let mut compress =
            zstd::Encoder::new(&mut compressed, zstd::DEFAULT_COMPRESSION_LEVEL).unwrap();
        t.serialize(&mut varbincode::Serializer::new(&mut compress))
            .unwrap();
        compress.finish().unwrap();
        if compressed.len() < uncompressed.len() {
            (compressed, true)
        } else {
            (uncompressed, false)
        }
    };
    let len = (data.len() + leb_len(ident) + leb_len(serial)) as u64;
    let len = if is_compressed {
        len | COMPRESSED_MASK
    } else {
        len
    };
    let mut frame = Vec::new();
    leb128::write::unsigned(&mut frame, len).unwrap();
    leb128::write::unsigned(&mut frame, serial).unwrap();
    leb128::write::unsigned(&mut frame, ident).unwrap();
    frame.extend_from_slice(&data);
    frame
}

fn leb_len(value: u64) -> usize {
    let mut v = Vec::new();
    leb128::write::unsigned(&mut v, value).unwrap()
}

/// Upstream's decode of a push's frame: a new zstd stream decoder for it
fn upstream_decode_push(frame: &[u8]) -> GetPaneRenderChangesResponse {
    let mut r = frame;
    let len = leb128::read::unsigned(&mut r).unwrap();
    let _serial = leb128::read::unsigned(&mut r).unwrap();
    let _ident = leb128::read::unsigned(&mut r).unwrap();
    assert!(len & COMPRESSED_MASK != 0);
    let mut decompress = zstd::Decoder::new(r).unwrap();
    serde::Deserialize::deserialize(&mut varbincode::Deserializer::new(&mut decompress)).unwrap()
}

#[cfg(windows)]
fn page_faults() -> u64 {
    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(
            process: isize,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }
    let mut c: ProcessMemoryCounters = unsafe { std::mem::zeroed() };
    c.cb = std::mem::size_of::<ProcessMemoryCounters>() as u32;
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    assert!(ok != 0, "K32GetProcessMemoryInfo failed");
    c.page_fault_count as u64
}

#[cfg(not(windows))]
fn page_faults() -> u64 {
    0
}

#[derive(Debug, Clone, Copy, Default)]
struct Cost {
    us_per_pdu: f64,
    faults_per_pdu: f64,
}

/// `f` run `n` times: its time and page faults a run
fn cost(n: usize, mut f: impl FnMut(usize)) -> Cost {
    let faults = page_faults();
    let start = Instant::now();
    for i in 0..n {
        f(i);
    }
    let elapsed = start.elapsed();
    Cost {
        us_per_pdu: elapsed.as_secs_f64() * 1e6 / n as f64,
        faults_per_pdu: (page_faults() - faults) as f64 / n as f64,
    }
}

/// The median of each field over the rounds
fn median(costs: &[Cost]) -> Cost {
    let pick = |f: fn(&Cost) -> f64| {
        let mut v: Vec<f64> = costs.iter().map(f).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    Cost {
        us_per_pdu: pick(|c| c.us_per_pdu),
        faults_per_pdu: pick(|c| c.faults_per_pdu),
    }
}

#[derive(Debug)]
struct Pair {
    what: String,
    bytes: usize,
    upstream: Cost,
    lbterm: Cost,
}

/// An AsyncRead over bytes that hands out all it's asked for, counting reads
#[derive(Debug)]
struct Counted {
    data: Vec<u8>,
    pos: usize,
    reads: usize,
}

impl smol::io::AsyncRead for Counted {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let me = &mut *self;
        me.reads += 1;
        let n = buf.len().min(me.data.len() - me.pos);
        buf[..n].copy_from_slice(&me.data[me.pos..me.pos + n]);
        me.pos += n;
        std::task::Poll::Ready(Ok(n))
    }
}

/// `frames` written down loopback TCP by a thread of their own and decoded
/// off the other end, upstream's way or with a PduReader: the time a PDU
fn over_tcp(frames: &[u8], count: usize, ours: bool) -> f64 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let bytes = frames.to_vec();
    let writer = std::thread::spawn(move || {
        use std::io::Write;
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        client.set_nodelay(true).unwrap();
        client.write_all(&bytes).unwrap();
    });
    let (accepted, _) = listener.accept().unwrap();
    let mut stream = smol::Async::new(accepted).unwrap();
    let mut reader = PduReader::new();
    let start = Instant::now();
    smol::block_on(async {
        for _ in 0..count {
            if ours {
                reader.decode_async(&mut stream, None).await.unwrap();
            } else {
                Pdu::decode_async(&mut stream, None).await.unwrap();
            }
        }
    });
    let us = start.elapsed().as_secs_f64() * 1e6 / count as f64;
    writer.join().unwrap();
    us
}

#[test]
fn lbterm_a_pdu_costs_no_zstd_context_and_one_read() {
    let scale: usize = std::env::var("LBTERM_CODEC_BENCH")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1);
    let n = 1000 * scale;
    let rounds = 3;
    let mut pairs = vec![];

    // Encoding pushes: a few lines changed, and a whole screen
    for rows in [3, 40] {
        let pushes: Vec<Pdu> = (0..64)
            .map(|i| Pdu::GetPaneRenderChangesResponse(push(rows, i)))
            .collect();
        let typed: Vec<GetPaneRenderChangesResponse> = (0..64).map(|i| push(rows, i)).collect();
        let mut bytes = Vec::new();
        pushes[0].encode(&mut bytes, 1).unwrap();
        assert_eq!(bytes, upstream_frame(25, 1, &typed[0]), "the same frame as upstream's");
        let (mut up, mut ours) = (vec![], vec![]);
        for _ in 0..rounds {
            up.push(cost(n, |i| {
                std::hint::black_box(upstream_frame(25, i as u64 + 1, &typed[i % 64]));
            }));
            ours.push(cost(n, |i| {
                let mut out = Vec::new();
                pushes[i % 64].encode(&mut out, i as u64 + 1).unwrap();
                std::hint::black_box(out);
            }));
        }
        pairs.push(Pair {
            what: format!("encode a push of {rows} lines"),
            bytes: bytes.len(),
            upstream: median(&up),
            lbterm: median(&ours),
        });
    }

    // Decoding a push of three lines
    let frames: Vec<Vec<u8>> = (0..64).map(|i| upstream_frame(25, 1, &push(3, i))).collect();
    let (mut up, mut ours) = (vec![], vec![]);
    for _ in 0..rounds {
        up.push(cost(n, |i| {
            std::hint::black_box(upstream_decode_push(&frames[i % 64]));
        }));
        ours.push(cost(n, |i| {
            std::hint::black_box(Pdu::decode(frames[i % 64].as_slice()).unwrap());
        }));
    }
    pairs.push(Pair {
        what: "decode a push of 3 lines".to_string(),
        bytes: frames[0].len(),
        upstream: median(&up),
        lbterm: median(&ours),
    });

    // Reading frames that came together: reads a PDU, and the time a PDU
    // over loopback TCP (a stand-in for the mux's AF_UNIX socket)
    let mut stream_bytes = Vec::new();
    for i in 0..n {
        stream_bytes.extend_from_slice(&frames[i % 64]);
    }
    let (upstream_reads, our_reads) = smol::block_on(async {
        let mut r = Counted {
            data: stream_bytes.clone(),
            pos: 0,
            reads: 0,
        };
        for _ in 0..n {
            Pdu::decode_async(&mut r, None).await.unwrap();
        }
        let upstream = r.reads;
        let mut r = Counted {
            data: stream_bytes.clone(),
            pos: 0,
            reads: 0,
        };
        let mut reader = PduReader::new();
        for _ in 0..n {
            reader.decode_async(&mut r, None).await.unwrap();
        }
        (upstream, r.reads)
    });
    let (mut up_tcp, mut our_tcp) = (vec![], vec![]);
    for _ in 0..rounds {
        up_tcp.push(over_tcp(&stream_bytes, n, false));
        our_tcp.push(over_tcp(&stream_bytes, n, true));
    }
    let med = |v: &mut Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };

    // By hand: codec has no JSON crate
    let cost_json = |c: &Cost| {
        format!(
            "{{\"us_per_pdu\": {:.3}, \"faults_per_pdu\": {:.3}}}",
            c.us_per_pdu, c.faults_per_pdu
        )
    };
    let pair_json: Vec<String> = pairs
        .iter()
        .map(|p| {
            format!(
                "    {{\"what\": \"{}\", \"bytes\": {}, \"upstream\": {}, \"lbterm\": {}}}",
                p.what,
                p.bytes,
                cost_json(&p.upstream),
                cost_json(&p.lbterm)
            )
        })
        .collect();
    let text = format!(
        "{{\n  \"pdus_a_round\": {n},\n  \"rounds\": {rounds},\n  \"pairs\": [\n{}\n  ],\n  \
         \"reads\": {{\"pdus\": {n}, \"frame_bytes\": {}, \"upstream_reads_per_pdu\": {:.3}, \
         \"lbterm_reads_per_pdu\": {:.4}, \"upstream_tcp_us_per_pdu\": {:.3}, \
         \"lbterm_tcp_us_per_pdu\": {:.3}}}\n}}\n",
        pair_json.join(",\n"),
        frames[0].len(),
        upstream_reads as f64 / n as f64,
        our_reads as f64 / n as f64,
        med(&mut up_tcp),
        med(&mut our_tcp),
    );
    println!("{text}");
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("lbterm-codec-cost.json");
    std::fs::write(&out, &text).unwrap();

    for pair in &pairs {
        assert!(
            pair.lbterm.faults_per_pdu < 10.0,
            "{}: {} page faults a PDU (upstream's way {}): a zstd context made for each PDU again?",
            pair.what,
            pair.lbterm.faults_per_pdu,
            pair.upstream.faults_per_pdu
        );
    }
    assert!(
        (our_reads as f64) < n as f64 / 10.0,
        "{} reads for {} PDUs that came together (upstream's way {})",
        our_reads,
        n,
        upstream_reads
    );
}
