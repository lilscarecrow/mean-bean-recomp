//! The sound driver against the Z80 of recordings (`.sndrec`, see the harness): every
//! FM and PSG write it made, in order (the DAC's sample bytes aside), and its RAM at
//! each frame's end. The driver is given the commands the 68000 sent (worked out from
//! the 68000's RAM in `<recording>.ramrec` the way `UpdateSound` picks them, plus a
//! test's pokes from `<recording>.cues`), and between them ticks; which comes first
//! when both are due is told by the writes. Frames that ended in the middle of a tick
//! aren't compared.

mod support;

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use bean_machine::audio::cube::{Cube, Write, MAILBOX};
use flate2::read::ZlibDecoder;

struct Frame {
    zram: Vec<u8>,
    /// (master clock, write)
    writes: Vec<(u32, Write)>,
    /// The DAC's last sample byte's clock.
    last_dac: Option<u32>,
}

fn sound_recordings() -> Vec<PathBuf> {
    let dir = std::env::var_os("MBM_SOUND_RECORDINGS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../bean-machine-re/sound_rec"));
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "sndrec"))
        .collect();
    files.sort();
    if files.is_empty() {
        eprintln!("skipping: no sound recordings in {} (set MBM_SOUND_RECORDINGS)", dir.display());
    }
    files
}

/// b"SNDREC1\0", u32 frames, per frame u32 length + zlib(Z80 RAM XOR the last frame's,
/// u32 n, n * (u32 clock, u8 chip, u8 port, u8 data, u8 0)).
fn frames(path: &Path) -> Vec<Frame> {
    let mut f = BufReader::new(File::open(path).unwrap());
    let mut head = [0u8; 12];
    f.read_exact(&mut head).unwrap();
    assert_eq!(&head[..8], b"SNDREC1\0");
    let count = u32::from_le_bytes(head[8..].try_into().unwrap());
    let mut zram = vec![0u8; 0x2000];
    let mut out = vec![];
    // The address latch: (part, register).
    let mut latch = (0u8, 0u8);
    for _ in 0..count {
        let mut len = [0u8; 4];
        f.read_exact(&mut len).unwrap();
        let mut blob = vec![0u8; u32::from_le_bytes(len) as usize];
        f.read_exact(&mut blob).unwrap();
        let mut raw = vec![];
        ZlibDecoder::new(&blob[..]).read_to_end(&mut raw).unwrap();
        for (z, d) in zram.iter_mut().zip(&raw) {
            *z ^= d;
        }
        let n = u32::from_le_bytes(raw[0x2000..0x2004].try_into().unwrap()) as usize;
        let mut writes = vec![];
        let mut last_dac = None;
        for e in raw[0x2004..0x2004 + n * 8].chunks(8) {
            let clock = u32::from_le_bytes(e[..4].try_into().unwrap());
            let (chip, port, data) = (e[4], e[5], e[6]);
            if chip == 1 {
                writes.push((clock, Write::Psg(data)));
            } else if port & 1 == 0 {
                latch = (port >> 1, data);
            } else if latch == (0, 0x2A) {
                last_dac = Some(clock);
            } else {
                writes.push((clock, Write::Fm { part: latch.0, reg: latch.1, data }));
            }
        }
        out.push(Frame { zram: zram.clone(), writes, last_dac });
    }
    out
}

fn cues(path: &Path) -> std::collections::HashMap<usize, u8> {
    let mut p = path.as_os_str().to_owned();
    p.push(".cues");
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?.parse().ok()?, u8::from_str_radix(it.next()?, 16).ok()?))
        })
        .collect()
}

/// What `UpdateSound` hands the Z80 at each frame's vertical blank: `sound_playing`
/// (or a test's poke into it), else the queue's next ID (the 68000 RAM of the frame
/// before, from `<recording>.ramrec`), when the frame had one (`frame_count` moved)
/// and the mailbox took it (else `sound_playing` keeps it).
fn commands(path: &Path, cues: &std::collections::HashMap<usize, u8>) -> Vec<(usize, u8)> {
    const PLAYING: u32 = 0xFF_1138;
    const QUEUED: u32 = 0xFF_113C;
    const CURRENT: u32 = 0xFF_113D;
    const QUEUE: u32 = 0xFF_113E;
    let rec = path.with_extension("ramrec");
    if !rec.exists() {
        // (no 68000 recording: the pokes alone)
        let mut out: Vec<(usize, u8)> = cues.iter().map(|(&f, &id)| (f, id)).collect();
        out.sort();
        return out;
    }
    let mut out = vec![];
    let mut prev: Option<support::Ram> = None;
    for (f, ram) in support::frames(&rec).enumerate() {
        if let Some(p) = &prev {
            if ram.u16(support::FRAME_COUNT) != p.u16(support::FRAME_COUNT) {
                let mut d0 = cues.get(&f).copied().unwrap_or(p.u8(PLAYING));
                if d0 == 0 && p.u8(QUEUED) != 0 {
                    d0 = p.u8(QUEUE + u32::from(p.u8(CURRENT)));
                }
                if d0 != 0 && ram.u8(PLAYING) != d0 {
                    out.push((f, d0));
                }
            }
        }
        prev = Some(ram);
    }
    out
}

/// Z80 RAM compared: all but the stack.
fn ram_differs(ours: &[u8], theirs: &[u8]) -> Option<usize> {
    (0..0x2000).filter(|a| !(0x1FC0..0x1FF0).contains(a)).find(|&a| ours[a] != theirs[a])
}

/// One poll of the main loop after an event: a song's own command, then the sample.
fn after_event(c: &mut Cube) {
    if c.ram[usize::from(MAILBOX)] != 0 && !c.paused {
        c.command();
    }
    c.sample_request();
}

#[derive(Clone, Copy, Debug)]
enum Event {
    Tick,
    Command(u8),
}

fn apply(c: &mut Cube, e: Event) {
    match e {
        Event::Tick => {
            if c.paused {
                return;
            }
            c.tick();
        }
        Event::Command(id) => {
            c.post(id);
            c.command();
        }
    }
    after_event(c);
}

fn describe(w: &[Write]) -> String {
    w.iter()
        .map(|w| match w {
            Write::Fm { part, reg, data } => format!("{part}:{reg:02X}={data:02X}"),
            Write::Psg(d) => format!("p{d:02X}"),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A frame in master clocks, and how long a tick may go on after its last write.
const FRAME: u32 = 896_040;
const TAIL: u32 = 40_000;

/// The commands' usual time in their frame (the vertical blank's `UpdateSound`).
const COMMAND_CLOCK: u32 = 120_000;

#[test]
fn the_driver_matches_the_z80() {
    if !support::install_rom() {
        return;
    }
    for path in sound_recordings() {
        let all = frames(&path);
        let cues = cues(&path);
        let sent = commands(&path, &cues);
        // The writes as one stream (a tick can run on into the next frame), each with
        // its frame and clock.
        let stream: Vec<(usize, u32, Write)> =
            all.iter().enumerate().flat_map(|(f, fr)| fr.writes.iter().map(move |&(clk, w)| (f, clk, w))).collect();
        let ends: Vec<usize> = all.iter().scan(0, |n, f| { *n += f.writes.len(); Some(*n) }).collect();
        let writes: Vec<Write> = stream.iter().map(|w| w.2).collect();
        let mut c = Cube::from_ram(&all[0].zram);
        let mut at = ends[0];
        let mut next_cmd = sent.iter().position(|&(f, _)| f >= 1).unwrap_or(sent.len());
        let mut frame = 1;
        let mut compared = 0;
        let trace = std::env::var_os("MBM_SOUND_TRACE").is_some();
        let fail = |c: &Cube, f: usize, at: usize, what: &str| -> ! {
            let got = &c.writes;
            let want = &writes[at..(at + got.len() + 8).min(writes.len())];
            let i = got.iter().zip(want).position(|(a, b)| a != b).unwrap_or(got.len().min(want.len()));
            let from = i.saturating_sub(6);
            panic!(
                "{}: frame {f}, {what}: writes differ at {i} of {}
  ROM {}
  our {}",
                path.display(),
                got.len(),
                describe(&want[from.min(want.len())..(i + 8).min(want.len())]),
                describe(&got[from..(i + 8).min(got.len())]),
            )
        };
        loop {
            // Frames ended here (with their commands in): compare the Z80 RAM.
            while frame < all.len() && at >= ends[frame] && sent.get(next_cmd).is_none_or(|&(f, _)| f > frame) {
                // (unless the frame ended in the middle of a tick, as the ROM's tick
                // counter or channel variables show,
                let z = &all[frame].zram;
                // or between a tick and the poll that takes its sample request,
                // and not too soon after the event's last write: a tick's end writes
                // nothing; with a sample playing, the DAC's next byte shows it ended)
                let busy = [0x14D1usize, 0x14D2, 0x14D6, 0x14E3].iter().any(|&a| z[a] != c.ram[a])
                    || (z[0x1FF8] != 0 && c.ram[0x1FF8] == 0)
                    || (at > 0 && stream[at - 1].0 == frame && stream[at - 1].1 > FRAME - TAIL)
                    || (at > 0 && stream[at - 1].0 == frame && all[frame].last_dac.is_some_and(|d| d < stream[at - 1].1));
                if at == ends[frame] && !busy {
                    if let Some(a) = ram_differs(&c.ram[..], &all[frame].zram) {
                        panic!("{}: frame {frame}: Z80 RAM ${a:04X}: ROM {:02X}, ours {:02X}", path.display(), all[frame].zram[a], c.ram[a]);
                    }
                    compared += 1;
                }
                frame += 1;
            }
            if frame >= all.len() {
                break;
            }
            let (wf, wclk) = stream.get(at).map_or((all.len(), 0), |w| (w.0, w.1));
            let cmd = sent.get(next_cmd).copied().filter(|&(f, _)| f <= wf);
            // Candidates, the likelier first.
            let mut tries: Vec<Vec<Event>> = vec![];
            match cmd {
                Some((f, id)) if f < wf || wclk >= COMMAND_CLOCK => {
                    tries.push(vec![Event::Command(id)]);
                    tries.push(vec![Event::Tick, Event::Command(id)]);
                }
                Some((_, id)) => {
                    tries.push(vec![Event::Tick, Event::Command(id)]);
                    tries.push(vec![Event::Command(id)]);
                }
                None => tries.push(vec![Event::Tick]),
            }
            let mut done = None;
            for t in &tries {
                let mut m = c.clone();
                m.writes.clear();
                for &e in t {
                    apply(&mut m, e);
                }
                let n = m.writes.len();
                if writes.get(at..at + n) == Some(&m.writes[..]) && (n > 0 || matches!(t[0], Event::Command(_))) {
                    done = Some((m, t.clone()));
                    break;
                }
            }
            let Some((m, events)) = done else {
                let mut m = c.clone();
                m.writes.clear();
                for &e in &tries[0] {
                    apply(&mut m, e);
                }
                fail(&m, wf, at, &format!("{:X?}", tries[0]));
            };
            if trace {
                eprintln!("frame {wf} ({wclk}): {events:X?}: writes {at}..{}", at + m.writes.len());
            }
            at += m.writes.len();
            if events.iter().any(|e| matches!(e, Event::Command(_))) {
                next_cmd += 1;
            }
            c = m;
            if at >= writes.len() && sent.get(next_cmd).is_none() {
                break;
            }
        }
        eprintln!("{}: the driver matches over {} frames ({compared} Z80 RAM snapshots)", path.display(), all.len());
    }
}
